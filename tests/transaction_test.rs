// A write spanning several queries runs in one transaction (MAIR-420): `state.get_smart_db().begin()`
// then `commit()`. Dropping the transaction (an early `?` return) or a failing query rolls back every
// query already run, so a crash or an error between two writes never leaves half of them applied.
// These tests are the worked example new APIs copy; they use a probe table of their own so they
// depend on no business table of the schema.

use mairie360_api_lib::database::db_interface::{ApiRequestDto, QueryParam};
use mairie360_api_lib::error::ApiLibError;
use mairie360_api_lib::smart_db::SmartDatabase;
use mairie360_api_lib::state::AppState;
use mairie360_api_lib::test_setup::queries_setup::get_shared_db;
use serde::Deserialize;
use serial_test::serial;

/// Nothing listens on port 1: these writes declare no cache key, so Redis is never used.
const UNREACHABLE_REDIS: &str = "redis://127.0.0.1:1";

#[derive(Deserialize)]
struct CreateProbeTable;

impl ApiRequestDto for CreateProbeTable {
    fn query_sql(&self) -> &'static str {
        "CREATE TABLE IF NOT EXISTS template_transaction_probe (label TEXT PRIMARY KEY)"
    }

    fn query_params(&self) -> &[QueryParam] {
        &[]
    }
}

#[derive(Deserialize)]
struct InsertProbe {
    params: Vec<QueryParam>,
}

impl InsertProbe {
    fn new(label: &str) -> Self {
        Self {
            params: vec![QueryParam::Text(label.to_owned())],
        }
    }
}

impl ApiRequestDto for InsertProbe {
    fn query_sql(&self) -> &'static str {
        "INSERT INTO template_transaction_probe (label) VALUES ($1)"
    }

    fn query_params(&self) -> &[QueryParam] {
        &self.params
    }
}

#[derive(Deserialize)]
struct CountProbes {
    params: Vec<QueryParam>,
}

impl CountProbes {
    fn with_prefix(prefix: &str) -> Self {
        Self {
            params: vec![QueryParam::Text(format!("{prefix}%"))],
        }
    }
}

impl ApiRequestDto for CountProbes {
    fn query_sql(&self) -> &'static str {
        "SELECT count(*) FROM template_transaction_probe WHERE label LIKE $1"
    }

    fn query_params(&self) -> &[QueryParam] {
        &self.params
    }
}

async fn state() -> AppState {
    let (_, pg_url) = get_shared_db().await;
    let state = AppState::new(UNREACHABLE_REDIS.to_owned(), pg_url.clone()).await;
    state
        .get_smart_db()
        .execute(CreateProbeTable)
        .await
        .expect("probe table");
    state
}

async fn count(db: &SmartDatabase, prefix: &str) -> i64 {
    db.fetch_scalar::<i64, _>(&CountProbes::with_prefix(prefix))
        .await
        .expect("count")
}

/// Two writes that must land together, the second one failing when `label_b` already exists.
async fn create_pair(db: &SmartDatabase, label_a: &str, label_b: &str) -> Result<(), ApiLibError> {
    let mut tx = db.begin().await?;
    tx.execute(&InsertProbe::new(label_a)).await?;
    tx.execute(&InsertProbe::new(label_b)).await?;
    tx.commit().await
}

#[actix_web::test]
#[serial]
async fn committed_writes_are_all_applied() {
    let state = state().await;
    let db = state.get_smart_db();

    create_pair(db, "commit-a", "commit-b")
        .await
        .expect("both writes");

    assert_eq!(count(db, "commit-").await, 2);
}

#[actix_web::test]
#[serial]
async fn a_failing_write_rolls_back_the_writes_before_it() {
    let state = state().await;
    let db = state.get_smart_db();
    db.execute(InsertProbe::new("rollback-taken"))
        .await
        .expect("seed");

    // The second insert hits the primary key: the `?` returns early and drops the transaction.
    let result = create_pair(db, "rollback-first", "rollback-taken").await;

    assert!(result.is_err());
    assert_eq!(count(db, "rollback-first").await, 0);
}

#[actix_web::test]
#[serial]
async fn an_explicit_rollback_discards_the_writes() {
    let state = state().await;
    let db = state.get_smart_db();

    let mut tx = db.begin().await.expect("begin");
    tx.execute(&InsertProbe::new("discarded"))
        .await
        .expect("insert");
    tx.rollback().await.expect("rollback");

    assert_eq!(count(db, "discarded").await, 0);
}
