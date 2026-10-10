use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use actix_web::{body::to_bytes, http::StatusCode, test, web, App};
use api_template::endpoints::health::{
    self, readiness, retry_until, wait_for_postgres, DEPENDENCY_TIMEOUT,
}; // change api name
use mairie360_api_lib::state::AppState;
use mairie360_api_lib::test_setup::queries_setup::get_shared_db;
use mairie360_api_lib::test_setup::redis_setup::start_redis_container;

/// Nothing listens on port 1: the connection is refused right away.
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

// A state with an unreachable Postgres cannot be built any more: since `mairie360_api_lib` 3.0 (MAIR-423)
// `AppState::new` panics when Postgres does not answer, which is what keeps an API without its database from
// starting. "Postgres is down" is therefore tested on the checks themselves (`readiness`, `retry_until`), and
// the wiring to the real Postgres and Redis by the tests that start a container.

#[actix_web::test]
async fn ready_answers_503_naming_every_unreachable_dependency() {
    // Neither check ever answers: each one is cut off by the timeout, concurrently.
    let started = Instant::now();
    let response = readiness(std::future::pending(), std::future::pending()).await;

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = to_bytes(response.into_body()).await.unwrap();
    assert_eq!(&body[..], b"not ready: postgres, redis");
    // Both checks run concurrently, each bounded by the timeout.
    assert!(started.elapsed() < DEPENDENCY_TIMEOUT * 2);
}

#[actix_web::test]
async fn ready_answers_200_only_when_both_checks_answer() {
    let up = readiness(async { true }, async { true }).await;
    assert_eq!(up.status(), StatusCode::OK);

    for (postgres, redis, down) in [
        (false, true, "postgres"),
        (true, false, "redis"),
        (false, false, "postgres, redis"),
    ] {
        let response = readiness(async move { postgres }, async move { redis }).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(response.into_body()).await.unwrap();
        assert_eq!(&body[..], format!("not ready: {down}").as_bytes());
    }
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
    // The liveness probe takes no `AppState` at all: mounted without any application data it still answers,
    // whatever the state of Postgres and Redis.
    let app = test::init_service(App::new().service(health::health)).await;
    let response =
        test::call_service(&app, test::TestRequest::get().uri("/health").to_request()).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(test::read_body(response).await, "OK");
}

#[actix_web::test]
async fn startup_check_passes_once_postgres_answers() {
    let (_, pg_url) = get_shared_db().await;
    let state = AppState::new(UNREACHABLE_REDIS.to_owned(), pg_url.clone()).await;

    assert!(wait_for_postgres(&state, 3, std::time::Duration::ZERO).await);
}

#[actix_web::test]
async fn startup_check_gives_up_after_its_attempts() {
    let calls = AtomicU32::new(0);

    let started = Instant::now();
    let answered = retry_until("test", 3, Duration::from_millis(10), || {
        calls.fetch_add(1, Ordering::SeqCst);
        async { false }
    })
    .await;

    assert!(!answered);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    // It sleeps between the attempts, not after the last one.
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[actix_web::test]
async fn startup_check_stops_at_the_first_answer() {
    let calls = AtomicU32::new(0);

    let answered = retry_until("test", 5, Duration::ZERO, || {
        let attempt = calls.fetch_add(1, Ordering::SeqCst) + 1;
        async move { attempt == 2 }
    })
    .await;

    assert!(answered);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
