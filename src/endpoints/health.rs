use std::future::Future;
use std::time::Duration;

use actix_web::{get, web, HttpResponse, Responder};
use mairie360_api_lib::state::AppState;
use utoipa::OpenApi;

use crate::database::ping::PingQueryView;

/// Longest time a dependency check may take before it counts as down. Keep the readiness probe's
/// `timeoutSeconds` above it.
pub const DEPENDENCY_TIMEOUT: Duration = Duration::from_secs(2);

/// Redis key read by the readiness probe: its value does not matter, only the round-trip does.
const REDIS_PROBE_KEY: &str = "readiness-probe";

/// Liveness probe: answers as long as the process serves HTTP, whatever the state of Postgres and
/// Redis, so that an outage of a dependency does not make Kubernetes restart every pod.
#[utoipa::path(
    get,
    path = "health",
    tag = "probes",
    summary = "Liveness probe",
    description = "Answers `200 OK` as long as the process serves HTTP. Checks no dependency: \
        use `GET /ready` to know whether Postgres and Redis are reachable.",
    responses(
        (status = 200, description = "The process is alive.", body = String,
            content_type = "text/plain", example = json!("OK"))
    )
)]
#[get("/health")]
pub async fn health() -> impl Responder {
    HttpResponse::Ok().body("OK")
}

/// Readiness probe: runs `SELECT 1` on Postgres and a read on Redis, each bounded by
/// [`DEPENDENCY_TIMEOUT`].
#[utoipa::path(
    get,
    path = "ready",
    tag = "probes",
    summary = "Readiness probe",
    description = "Runs `SELECT 1` on Postgres and reads a key on Redis, each bounded by 2 seconds. \
        Answers `200` when both answer, `503` naming the unreachable dependencies otherwise. \
        Kubernetes stops routing traffic to a pod that is not ready, without restarting it.",
    responses(
        (status = 200, description = "Postgres and Redis both answered.", body = String,
            content_type = "text/plain", example = json!("ready")),
        (status = 503, description = "Postgres, Redis or both did not answer within 2 seconds; \
            the body lists them.", body = String, content_type = "text/plain",
            example = json!("not ready: postgres, redis"))
    )
)]
#[get("/ready")]
pub async fn ready(state: web::Data<AppState>) -> impl Responder {
    readiness(postgres_probe(&state), redis_probe(&state)).await
}

/// Runs the two dependency checks concurrently, each bounded by [`DEPENDENCY_TIMEOUT`], and answers
/// `200 ready` or `503 not ready: <unreachable dependencies>`.
///
/// The checks are passed in rather than read from an [`AppState`] so that "a dependency is down" can be
/// tested: since `mairie360_api_lib` 3.0 (MAIR-423) `AppState::new` panics when Postgres is
/// unreachable, so a state with a dead database cannot be built any more.
pub async fn readiness(
    postgres: impl Future<Output = bool>,
    redis: impl Future<Output = bool>,
) -> HttpResponse {
    let (postgres, redis) = tokio::join!(within_timeout(postgres), within_timeout(redis));

    let down: Vec<&str> = [("postgres", postgres), ("redis", redis)]
        .into_iter()
        .filter_map(|(name, up)| (!up).then_some(name))
        .collect();
    if down.is_empty() {
        HttpResponse::Ok().body("ready")
    } else {
        HttpResponse::ServiceUnavailable().body(format!("not ready: {}", down.join(", ")))
    }
}

/// `SELECT 1` on Postgres, unbounded: [`readiness`] and [`postgres_answers`] bound it.
async fn postgres_probe(state: &AppState) -> bool {
    state
        .get_smart_db()
        .fetch_scalar::<i32, _>(&PingQueryView)
        .await
        .is_ok()
}

/// One read on Redis, unbounded: [`readiness`] bounds it.
async fn redis_probe(state: &AppState) -> bool {
    state.get_redis().key_exist(REDIS_PROBE_KEY).await.is_ok()
}

/// `SELECT 1` on Postgres, bounded by [`DEPENDENCY_TIMEOUT`].
pub async fn postgres_answers(state: &AppState) -> bool {
    within_timeout(postgres_probe(state)).await
}

/// Startup check: tries [`postgres_answers`] up to `attempts` times, `delay` apart, and says whether
/// Postgres ever answered. `main.rs` refuses to start when it does not, instead of serving `500`s.
pub async fn wait_for_postgres(state: &AppState, attempts: u32, delay: Duration) -> bool {
    retry_until("Postgres", attempts, delay, || postgres_answers(state)).await
}

/// Calls `check` up to `attempts` times, `delay` apart, and says whether it ever answered `true`.
pub async fn retry_until<F, Fut>(what: &str, attempts: u32, delay: Duration, mut check: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    for attempt in 1..=attempts {
        if check().await {
            return true;
        }
        tracing::warn!(attempt, attempts, "{what} does not answer yet");
        if attempt < attempts {
            tokio::time::sleep(delay).await;
        }
    }
    false
}

async fn within_timeout(check: impl Future<Output = bool>) -> bool {
    tokio::time::timeout(DEPENDENCY_TIMEOUT, check)
        .await
        .unwrap_or(false)
}

#[derive(OpenApi)]
#[openapi(paths(health, ready))]
pub struct HealthDoc;
