//! The permanent scan (MAIR-498): runs the deterministic database scan, journals every finding
//! (counts and locations only) and returns them. Started on demand (`POST /api/v1/scans`) and
//! every `SCAN_INTERVAL_SECONDS` (6 hours by default) by `main.rs`.

use crate::store::{ComplianceStore, Finding, JournalEntry, StoreError};

/// Default period of the background scan.
pub const DEFAULT_SCAN_INTERVAL_SECONDS: u64 = 6 * 3600;

/// Runs one database scan and journals its findings.
///
/// # Errors
///
/// The store's error when the scan cannot run; a failed journal write is returned too, so that a
/// finding is never silently lost.
pub async fn run_database_scan(store: &dyn ComplianceStore) -> Result<Vec<Finding>, StoreError> {
    let findings = store.scan().await?;
    for finding in &findings {
        store.journal(&JournalEntry::finding(finding)).await?;
    }
    tracing::info!(findings = findings.len(), "database compliance scan done");
    Ok(findings)
}

/// The period of the background scan, from `SCAN_INTERVAL_SECONDS` (0 disables it).
#[must_use]
pub fn scan_interval(value: Option<&str>) -> Option<std::time::Duration> {
    let seconds = value
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_SCAN_INTERVAL_SECONDS);
    (seconds > 0).then(|| std::time::Duration::from_secs(seconds))
}

#[cfg(test)]
pub mod tests {
    use super::{run_database_scan, scan_interval};
    use crate::store::{Action, ComplianceStore, Finding, JournalEntry, Storage, StoreError};
    use async_trait::async_trait;
    use std::sync::Mutex;

    /// In-memory store for the tests of the scan and the erasure.
    #[derive(Default)]
    pub struct FakeStore {
        pub findings: Vec<Finding>,
        pub journal: Mutex<Vec<JournalEntry>>,
    }

    #[async_trait]
    impl ComplianceStore for FakeStore {
        async fn scan(&self) -> Result<Vec<Finding>, StoreError> {
            Ok(self.findings.clone())
        }

        async fn journal(&self, entry: &JournalEntry) -> Result<(), StoreError> {
            self.journal.lock().unwrap().push(entry.clone());
            Ok(())
        }
    }

    #[actix_web::test]
    async fn every_finding_is_journaled_without_value() {
        let store = FakeStore {
            findings: vec![Finding {
                kind: "retention_overdue".into(),
                location: "sessions".into(),
                rows: 3,
                detail: "older than 6 mons".into(),
            }],
            ..FakeStore::default()
        };
        let findings = run_database_scan(&store).await.unwrap();
        assert_eq!(findings.len(), 1);
        let journal = store.journal.lock().unwrap();
        assert_eq!(journal.len(), 1);
        assert_eq!(journal[0].storage, Storage::Database);
        assert_eq!(journal[0].action, Action::Detected);
        assert_eq!(journal[0].rows, Some(3));
        assert_eq!(journal[0].masked_excerpt, None);
    }

    #[test]
    fn the_interval_defaults_to_six_hours_and_zero_disables_it() {
        assert_eq!(scan_interval(None).unwrap().as_secs(), 21_600);
        assert_eq!(scan_interval(Some("60")).unwrap().as_secs(), 60);
        assert_eq!(scan_interval(Some("0")), None);
    }
}
