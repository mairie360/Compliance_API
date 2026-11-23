//! The erasure of a user has no Keycloak step any more: `ensure_steps` creates the five steps of
//! Resend, the database, S3, Redis and the backup key, whatever the Database release (the `keycloak`
//! step is refused by v4.0.0 and ignored here before it). Runs as `compliance_api`, like in staging.

use compliance_api::store::pg::PgStore;
use compliance_api::store::{ComplianceStore, Step};
use mairie360_api_lib::state::AppState;
use mairie360_api_lib::test_setup::queries_setup::{get_shared_db_as, ADMIN_ID};
use serial_test::serial;

const API_ROLE: &str = "compliance_api";
/// Nothing listens on port 1: these queries declare no cache key, so Redis is never used.
const UNREACHABLE_REDIS: &str = "redis://127.0.0.1:1";

#[actix_web::test]
#[serial]
async fn the_steps_of_an_erasure_do_not_include_keycloak() {
    let state = AppState::new(
        UNREACHABLE_REDIS.to_owned(),
        get_shared_db_as(API_ROLE).await,
    )
    .await;
    let store = PgStore::new(state.get_smart_db().clone());
    let user_id = *ADMIN_ID.get().unwrap();

    store.ensure_steps(user_id).await.expect("create the steps");
    let steps = store.steps(user_id).await.expect("read the steps");

    let mut names: Vec<&str> = steps.iter().map(|s| s.step.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["backup_key", "database", "redis", "resend", "s3"],
        "{steps:?}"
    );
    assert!(Step::ORDER.iter().all(|s| s.as_str() != "keycloak"));
}
