//! The code under test connects as `compliance_api`, never as the superuser (MAIR-556): a
//! superuser ignores the grants of Database's `api_grants.sql`, so a query the API may not run in
//! staging would pass here.

use mairie360_api_lib::database::db_interface::{ApiRequestDto, QueryParam};
use mairie360_api_lib::state::AppState;
use mairie360_api_lib::test_setup::queries_setup::get_shared_db_as;
use serde::{Deserialize, Serialize};
use serial_test::serial;

/// The Postgres role the API connects with in staging.
const API_ROLE: &str = "compliance_api"; // change api name
/// Nothing listens on port 1: these queries declare no cache key, so Redis is never used.
const UNREACHABLE_REDIS: &str = "redis://127.0.0.1:1";

/// Who the connection runs as.
#[derive(Deserialize)]
struct CurrentRole;

impl ApiRequestDto for CurrentRole {
    fn query_sql(&self) -> &'static str {
        "SELECT to_jsonb(t) FROM (
            SELECT current_user::TEXT AS name, rolsuper AS is_superuser
            FROM pg_roles WHERE rolname = current_user
        ) t"
    }

    fn query_params(&self) -> &[QueryParam] {
        &[]
    }
}

#[derive(Deserialize, Serialize)]
struct Role {
    name: String,
    is_superuser: bool,
}

/// A table of another domain: `compliance_api` has no grant on `projects`.
#[derive(Deserialize)]
struct CountProjects;

impl ApiRequestDto for CountProjects {
    fn query_sql(&self) -> &'static str {
        "SELECT count(*) FROM projects"
    }

    fn query_params(&self) -> &[QueryParam] {
        &[]
    }
}

/// The state the endpoints of the API get in the tests.
async fn api_state() -> AppState {
    AppState::new(
        UNREACHABLE_REDIS.to_owned(),
        get_shared_db_as(API_ROLE).await,
    )
    .await
}

#[actix_web::test]
#[serial]
async fn the_code_under_test_runs_as_the_api_role() {
    let state = api_state().await;

    let role = state
        .get_smart_db()
        .fetch_one::<Role, _>(&CurrentRole)
        .await
        .expect("current role");

    assert_eq!(role.name, API_ROLE);
    assert!(!role.is_superuser);
}

#[actix_web::test]
#[serial]
async fn the_api_role_is_held_to_its_grants() {
    let state = api_state().await;

    let error = state
        .get_smart_db()
        .fetch_scalar::<i64, _>(&CountProjects)
        .await
        .expect_err("compliance_api must not read projects");

    // 42501 = insufficient_privilege.
    assert!(
        error.to_string().contains("SQLSTATE 42501"),
        "expected a permission denial, got {error}"
    );
}
