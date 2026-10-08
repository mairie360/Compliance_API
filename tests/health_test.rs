use std::time::Instant;

use actix_web::{http::StatusCode, test, web, App};
use compliance_api::endpoints::health::{self, wait_for_postgres, DEPENDENCY_TIMEOUT};
use mairie360_api_lib::state::AppState;
use mairie360_api_lib::test_setup::db_setup::start_postgres_container;
use mairie360_api_lib::test_setup::queries_setup::get_shared_db;
use mairie360_api_lib::test_setup::redis_setup::start_redis_container;

/// Nothing listens on port 1: the connection is refused right away.
const UNREACHABLE_REDIS: &str = "redis://127.0.0.1:1";

/// An `AppState` whose Postgres is gone. Since lib 3.0.0 `AppState::new` panics when Postgres does
/// not answer, so the state is built on a dedicated database that is stopped afterwards (the
/// shared one is used by the other tests).
async fn state_without_postgres() -> AppState {
    let (postgres, db) = start_postgres_container().await;
    let pg_url = format!(
        "postgres://postgres:postgres@{}:{}/postgres",
        db.host, db.port
    );
    let state = AppState::new(UNREACHABLE_REDIS.to_owned(), pg_url).await;
    postgres.stop().await.expect("stop the Postgres container");
    state
}

async fn get(state: AppState, uri: &str) -> (StatusCode, String) {
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .service(health::health)
            .service(health::ready),
    )
    .await;
    let response = test::call_service(&app, test::TestRequest::get().uri(uri).to_request()).await;
    let status = response.status();
    let body = test::read_body(response).await;
    (status, String::from_utf8(body.to_vec()).unwrap())
}

#[actix_web::test]
async fn ready_answers_200_when_postgres_and_redis_answer() {
    let (_, pg_url) = get_shared_db().await;
    let (_redis, redis) = start_redis_container().await;
    let state = AppState::new(redis.url.clone(), pg_url.clone()).await;

    assert_eq!(
        get(state, "/ready").await,
        (StatusCode::OK, "ready".to_owned())
    );
}

#[actix_web::test]
async fn ready_answers_503_naming_every_unreachable_dependency() {
    let state = state_without_postgres().await;

    let started = Instant::now();
    let answer = get(state, "/ready").await;

    assert_eq!(
        answer,
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "not ready: postgres, redis".to_owned()
        )
    );
    // Both checks run concurrently, each bounded by the timeout.
    assert!(started.elapsed() < DEPENDENCY_TIMEOUT * 2);
}

#[actix_web::test]
async fn ready_names_only_the_unreachable_dependency() {
    let (_, pg_url) = get_shared_db().await;
    let state = AppState::new(UNREACHABLE_REDIS.to_owned(), pg_url.clone()).await;

    assert_eq!(
        get(state, "/ready").await,
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "not ready: redis".to_owned()
        )
    );
}

#[actix_web::test]
async fn health_answers_200_even_without_dependencies() {
    let state = state_without_postgres().await;

    assert_eq!(
        get(state, "/health").await,
        (StatusCode::OK, "OK".to_owned())
    );
}

#[actix_web::test]
async fn startup_check_passes_once_postgres_answers() {
    let (_, pg_url) = get_shared_db().await;
    let state = AppState::new(UNREACHABLE_REDIS.to_owned(), pg_url.clone()).await;

    assert!(wait_for_postgres(&state, 3, std::time::Duration::ZERO).await);
}

#[actix_web::test]
async fn startup_check_gives_up_after_its_attempts() {
    let state = state_without_postgres().await;

    let started = Instant::now();
    assert!(!wait_for_postgres(&state, 2, std::time::Duration::from_millis(10)).await);
    assert!(started.elapsed() < DEPENDENCY_TIMEOUT * 2 + std::time::Duration::from_secs(1));
}
