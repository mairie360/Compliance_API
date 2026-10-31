//! `PgStore::journal_all` writes the findings of a scan in one transaction (review of #5, MAIR-420):
//! when one entry is refused, none of the batch stays, so a retried scan never journals the same
//! finding twice. Runs as `compliance_api`, like in staging (MAIR-556).

use compliance_api::store::pg::PgStore;
use compliance_api::store::{Action, ComplianceStore, JournalEntry, Storage};
use mairie360_api_lib::database::db_interface::{ApiRequestDto, QueryParam};
use mairie360_api_lib::state::AppState;
use mairie360_api_lib::test_setup::queries_setup::get_shared_db_as;
use serde::Deserialize;
use serial_test::serial;

const API_ROLE: &str = "compliance_api";
/// Nothing listens on port 1: these queries declare no cache key, so Redis is never used.
const UNREACHABLE_REDIS: &str = "redis://127.0.0.1:1";

#[derive(Deserialize)]
struct CountAt {
    params: Vec<QueryParam>,
}

impl ApiRequestDto for CountAt {
    fn query_sql(&self) -> &'static str {
        "SELECT to_jsonb(count(*)) FROM compliance_journal WHERE location = $1"
    }

    fn query_params(&self) -> &[QueryParam] {
        &self.params
    }
}

fn entry(kind: &str, location: &str) -> JournalEntry {
    JournalEntry {
        kind: kind.to_owned(),
        storage: Storage::Database,
        location: location.to_owned(),
        action: Action::Detected,
        rows: Some(1),
        masked_excerpt: None,
        cause_hint: None,
        user_id: None,
    }
}

async fn store_and_state() -> (PgStore, AppState) {
    let state = AppState::new(
        UNREACHABLE_REDIS.to_owned(),
        get_shared_db_as(API_ROLE).await,
    )
    .await;
    (PgStore::new(state.get_smart_db().clone()), state)
}

async fn count(state: &AppState, location: &str) -> i64 {
    state
        .get_smart_db()
        .fetch_one::<i64, _>(&CountAt {
            params: vec![QueryParam::Text(location.to_owned())],
        })
        .await
        .expect("count the journal")
}

#[actix_web::test]
#[serial]
async fn a_batch_is_journaled_whole() {
    let (store, state) = store_and_state().await;
    let location = "journal_test.whole";
    store
        .journal_all(&[
            entry("retention_overdue", location),
            entry("erased_user_data", location),
        ])
        .await
        .expect("both entries are valid");
    assert_eq!(count(&state, location).await, 2);
}

#[actix_web::test]
#[serial]
async fn a_refused_entry_leaves_none_of_its_batch() {
    let (store, state) = store_and_state().await;
    let location = "journal_test.rollback";
    // `kind` must match ^[a-z][a-z0-9_]*$: the second entry is refused by the CHECK constraint.
    let result = store
        .journal_all(&[
            entry("retention_overdue", location),
            entry("Not A Kind", location),
        ])
        .await;
    assert!(result.is_err());
    assert_eq!(count(&state, location).await, 0);
}
