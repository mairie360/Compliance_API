use std::time::Instant;

use actix_web::{http::StatusCode, test, web, App};
use api_template::endpoints::health::{self, wait_for_postgres, DEPENDENCY_TIMEOUT}; // change api name
use mairie360_api_lib::state::AppState;
use mairie360_api_lib::test_setup::queries_setup::get_shared_db;
use mairie360_api_lib::test_setup::redis_setup::start_redis_container;

/// Nothing listens on port 1: both connections are refused right away.
const UNREACHABLE_PG: &str = "postgres://postgres:postgres@127.0.0.1:1/postgres";
const UNREACHABLE_REDIS: &str = "redis://127.0.0.1:1";

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
    let state = AppState::new(UNREACHABLE_REDIS.to_owned(), UNREACHABLE_PG.to_owned()).await;

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
    let state = AppState::new(UNREACHABLE_REDIS.to_owned(), UNREACHABLE_PG.to_owned()).await;

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
    let state = AppState::new(UNREACHABLE_REDIS.to_owned(), UNREACHABLE_PG.to_owned()).await;

    let started = Instant::now();
    assert!(!wait_for_postgres(&state, 2, std::time::Duration::from_millis(10)).await);
    assert!(started.elapsed() < DEPENDENCY_TIMEOUT * 2 + std::time::Duration::from_secs(1));
}
