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
    let postgres = postgres_answers(&state);
    let redis =
        within_timeout(async { state.get_redis().key_exist(REDIS_PROBE_KEY).await.is_ok() });
    let (postgres, redis) = tokio::join!(postgres, redis);

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

/// `SELECT 1` on Postgres, bounded by [`DEPENDENCY_TIMEOUT`].
pub async fn postgres_answers(state: &AppState) -> bool {
    within_timeout(async {
        state
            .get_smart_db()
            .fetch_scalar::<i32, _>(&PingQueryView)
            .await
            .is_ok()
    })
    .await
}

/// Startup check: tries [`postgres_answers`] up to `attempts` times, `delay` apart, and says whether
/// Postgres ever answered. `main.rs` refuses to start when it does not, instead of serving `500`s.
pub async fn wait_for_postgres(state: &AppState, attempts: u32, delay: Duration) -> bool {
    for attempt in 1..=attempts {
        if postgres_answers(state).await {
            return true;
        }
        tracing::warn!(attempt, attempts, "Postgres does not answer yet");
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
