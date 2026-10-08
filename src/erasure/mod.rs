//! Centralized erasure of a user (MAIR-498): the compliance service is the only one holding the
//! admin rights of Keycloak and Resend and the erasure rights on S3 and Redis. An erasure is a set
//! of steps (`erasure_steps`), each idempotent, journaled as proof (`compliance_journal`, without
//! the data) and retried until done:
//!
//! 1. Keycloak and Resend, which need the e-mail / Keycloak subject (`fn_erasure_targets`);
//! 2. the database (`anonymize_user`, MAIR-289), once those are done since it clears them;
//! 3. S3, Redis and the backup key (MAIR-500), keyed by the user id.

pub mod connectors;

use crate::store::{
    Action, ComplianceStore, JournalEntry, Step, StepState, StepStatus, StoreError,
};
use connectors::{Connector, Connectors, ErasureContext, Outcome};
use std::collections::HashMap;

/// Default period of the retry of the unfinished erasures.
pub const DEFAULT_RETRY_SECONDS: u64 = 300;

fn connector(connectors: &Connectors, step: Step) -> Option<&dyn Connector> {
    match step {
        Step::Keycloak => Some(connectors.keycloak.as_ref()),
        Step::Resend => Some(connectors.resend.as_ref()),
        Step::S3 => Some(connectors.s3.as_ref()),
        Step::Redis => Some(connectors.redis.as_ref()),
        Step::BackupKey => Some(connectors.backup_key.as_ref()),
        Step::Database => None,
    }
}

fn entry(user_id: i32, step: Step, action: Action, hint: Option<&str>) -> JournalEntry {
    JournalEntry {
        kind: if hint == Some(NOT_CONFIGURED) {
            "erasure_skipped".into()
        } else {
            "erasure_step".into()
        },
        storage: step.storage(),
        location: step.as_str().into(),
        action,
        rows: None,
        masked_excerpt: None,
        cause_hint: hint.map(str::to_owned),
        user_id: Some(user_id),
    }
}

const NOT_CONFIGURED: &str = "not configured on this instance";

/// Runs (or resumes) the erasure of `user_id` and returns its steps.
///
/// # Errors
///
/// A store failure; a failing step is recorded (`failed`, reason) and retried later instead.
pub async fn run_erasure(
    store: &dyn ComplianceStore,
    connectors: &Connectors,
    user_id: i32,
) -> Result<Vec<StepState>, StoreError> {
    store.ensure_steps(user_id).await?;
    let mut status: HashMap<Step, StepStatus> = store
        .steps(user_id)
        .await?
        .into_iter()
        .map(|s| (s.step, s.status))
        .collect();
    let mut targets = store.erasure_targets(user_id).await?;
    for step in Step::ORDER {
        if status.get(&step) == Some(&StepStatus::Done) {
            continue;
        }
        let result = if step == Step::Database {
            let ready = Step::ORDER
                .iter()
                .filter(|s| s.needs_targets())
                .all(|s| status.get(s) == Some(&StepStatus::Done));
            if !ready {
                // Stays pending: the database step clears what the earlier steps still need.
                continue;
            }
            store
                .anonymize(user_id)
                .await
                .map(|()| Outcome::Erased)
                .map_err(|e| e.0)
        } else {
            let Some(connector) = connector(connectors, step) else {
                continue;
            };
            let context = ErasureContext {
                user_id,
                targets: targets.clone(),
            };
            connector.erase(&context).await
        };
        let (new_status, action, hint) = match &result {
            Ok(Outcome::Erased) => (StepStatus::Done, Action::Erased, None),
            Ok(Outcome::NotConfigured) => {
                (StepStatus::Done, Action::Detected, Some(NOT_CONFIGURED))
            }
            Err(reason) => (StepStatus::Failed, Action::Failed, Some(reason.as_str())),
        };
        store.record_step(user_id, step, new_status, hint).await?;
        store.journal(&entry(user_id, step, action, hint)).await?;
        if new_status == StepStatus::Failed {
            tracing::warn!(
                user_id,
                step = step.as_str(),
                reason = hint.unwrap_or_default(),
                "erasure step failed, retried later"
            );
        }
        if step == Step::Database && new_status == StepStatus::Done {
            // The account no longer holds them: the steps after the database run by user id only.
            targets = None;
        }
        status.insert(step, new_status);
    }
    store.steps(user_id).await
}

/// Resumes every unfinished erasure (background retry of `main.rs`).
///
/// # Errors
///
/// The store's error when the unfinished erasures cannot be listed.
pub async fn retry_unfinished(
    store: &dyn ComplianceStore,
    connectors: &Connectors,
) -> Result<usize, StoreError> {
    let users = store.unfinished_erasures().await?;
    for user_id in &users {
        if let Err(error) = run_erasure(store, connectors, *user_id).await {
            tracing::error!(user_id, error = %error, "erasure retry failed");
        }
    }
    Ok(users.len())
}

#[cfg(test)]
mod tests {
    use super::connectors::{Connector, Connectors, ErasureContext, Outcome};
    use super::{retry_unfinished, run_erasure};
    use crate::store::{
        Action, ComplianceStore, ErasureTargets, Finding, JournalEntry, Step, StepState,
        StepStatus, StoreError,
    };
    use async_trait::async_trait;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MemoryStore {
        steps: Mutex<BTreeMap<(i32, String), StepState>>,
        journal: Mutex<Vec<JournalEntry>>,
        anonymized: Mutex<bool>,
    }

    #[async_trait]
    impl ComplianceStore for MemoryStore {
        async fn scan(&self) -> Result<Vec<Finding>, StoreError> {
            Ok(vec![])
        }
        async fn journal(&self, entry: &JournalEntry) -> Result<(), StoreError> {
            self.journal.lock().unwrap().push(entry.clone());
            Ok(())
        }
        async fn user_exists(&self, user_id: i32) -> Result<bool, StoreError> {
            Ok(user_id == 42)
        }
        async fn erasure_targets(
            &self,
            _user_id: i32,
        ) -> Result<Option<ErasureTargets>, StoreError> {
            Ok((!*self.anonymized.lock().unwrap()).then(|| ErasureTargets {
                email: "jane@example.com".into(),
                keycloak_subject: Some("kc-1".into()),
            }))
        }
        async fn ensure_steps(&self, user_id: i32) -> Result<(), StoreError> {
            let mut steps = self.steps.lock().unwrap();
            for step in Step::ORDER {
                steps
                    .entry((user_id, step.as_str().into()))
                    .or_insert(StepState {
                        step,
                        status: StepStatus::Pending,
                        attempts: 0,
                        last_error: None,
                    });
            }
            Ok(())
        }
        async fn steps(&self, user_id: i32) -> Result<Vec<StepState>, StoreError> {
            Ok(self
                .steps
                .lock()
                .unwrap()
                .iter()
                .filter(|((u, _), _)| *u == user_id)
                .map(|(_, s)| s.clone())
                .collect())
        }
        async fn record_step(
            &self,
            user_id: i32,
            step: Step,
            status: StepStatus,
            error: Option<&str>,
        ) -> Result<(), StoreError> {
            let mut steps = self.steps.lock().unwrap();
            let state = steps.get_mut(&(user_id, step.as_str().into())).unwrap();
            state.status = status;
            state.attempts += 1;
            state.last_error = error.map(str::to_owned);
            Ok(())
        }
        async fn unfinished_erasures(&self) -> Result<Vec<i32>, StoreError> {
            let mut users: Vec<i32> = self
                .steps
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, s)| s.status != StepStatus::Done)
                .map(|((u, _), _)| *u)
                .collect();
            users.dedup();
            Ok(users)
        }
        async fn anonymize(&self, _user_id: i32) -> Result<(), StoreError> {
            *self.anonymized.lock().unwrap() = true;
            Ok(())
        }
    }

    /// Fails the first `failures` calls, then erases; records whether it saw the targets.
    struct FakeConnector {
        failures: AtomicUsize,
        calls: Arc<AtomicUsize>,
        saw_targets: Arc<Mutex<Vec<bool>>>,
        outcome: Outcome,
    }

    impl FakeConnector {
        fn boxed(
            failures: usize,
            outcome: Outcome,
            calls: &Arc<AtomicUsize>,
            saw: &Arc<Mutex<Vec<bool>>>,
        ) -> Box<dyn Connector> {
            Box::new(Self {
                failures: AtomicUsize::new(failures),
                calls: Arc::clone(calls),
                saw_targets: Arc::clone(saw),
                outcome,
            })
        }
    }

    #[async_trait]
    impl Connector for FakeConnector {
        async fn erase(&self, context: &ErasureContext) -> Result<Outcome, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.saw_targets
                .lock()
                .unwrap()
                .push(context.targets.is_some());
            if self.failures.load(Ordering::SeqCst) > 0 {
                self.failures.fetch_sub(1, Ordering::SeqCst);
                return Err("service answered 503".into());
            }
            Ok(self.outcome)
        }
    }

    fn connectors(
        keycloak_failures: usize,
    ) -> (Connectors, Arc<AtomicUsize>, Arc<Mutex<Vec<bool>>>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let saw = Arc::new(Mutex::new(vec![]));
        let connectors = Connectors {
            keycloak: FakeConnector::boxed(keycloak_failures, Outcome::Erased, &calls, &saw),
            resend: FakeConnector::boxed(0, Outcome::Erased, &calls, &saw),
            s3: FakeConnector::boxed(0, Outcome::Erased, &calls, &saw),
            redis: FakeConnector::boxed(0, Outcome::Erased, &calls, &saw),
            backup_key: FakeConnector::boxed(0, Outcome::NotConfigured, &calls, &saw),
        };
        (connectors, calls, saw)
    }

    #[actix_web::test]
    async fn every_step_runs_once_and_is_journaled_as_proof() {
        let store = MemoryStore::default();
        let (connectors, calls, saw) = connectors(0);
        let steps = run_erasure(&store, &connectors, 42).await.unwrap();
        assert!(
            steps.iter().all(|s| s.status == StepStatus::Done),
            "{steps:?}"
        );
        assert!(*store.anonymized.lock().unwrap());
        // Keycloak and Resend saw the targets, S3 / Redis / backup key ran after the anonymization.
        assert_eq!(*saw.lock().unwrap(), vec![true, true, false, false, false]);
        let journal = store.journal.lock().unwrap().clone();
        assert_eq!(journal.len(), 6);
        assert!(journal
            .iter()
            .all(|e| e.user_id == Some(42) && e.masked_excerpt.is_none()));
        assert_eq!(
            journal
                .iter()
                .filter(|e| e.kind == "erasure_skipped")
                .count(),
            1,
            "the backup key is not configured yet"
        );
        // A second run does nothing: every step is done.
        run_erasure(&store, &connectors, 42).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 5);
    }

    #[actix_web::test]
    async fn a_failing_step_blocks_the_database_until_a_retry_succeeds() {
        let store = MemoryStore::default();
        let (connectors, _, _) = connectors(1);
        let steps = run_erasure(&store, &connectors, 42).await.unwrap();
        let keycloak = steps.iter().find(|s| s.step == Step::Keycloak).unwrap();
        assert_eq!(keycloak.status, StepStatus::Failed);
        assert_eq!(keycloak.last_error.as_deref(), Some("service answered 503"));
        assert_eq!(
            steps
                .iter()
                .find(|s| s.step == Step::Database)
                .unwrap()
                .status,
            StepStatus::Pending
        );
        assert!(
            !*store.anonymized.lock().unwrap(),
            "the database step waits for Keycloak"
        );
        assert!(store
            .journal
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.action == Action::Failed));

        assert_eq!(retry_unfinished(&store, &connectors).await.unwrap(), 1);
        let steps = store.steps(42).await.unwrap();
        assert!(
            steps.iter().all(|s| s.status == StepStatus::Done),
            "{steps:?}"
        );
        assert!(*store.anonymized.lock().unwrap());
        assert_eq!(
            steps
                .iter()
                .find(|s| s.step == Step::Keycloak)
                .unwrap()
                .attempts,
            2
        );
    }
}
