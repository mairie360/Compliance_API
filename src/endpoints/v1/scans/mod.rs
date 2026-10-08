//! `POST /api/v1/scans` (MAIR-498): runs the database scan now (service token only).

use crate::auth::ServiceCaller;
use crate::scan::run_scans;
use crate::store::pg::PgStore;
use crate::store::Finding;
use actix_web::{post, web, HttpResponse};
use mairie360_api_lib::state::AppState;
#[utoipa::path(
    post,
    path = "/scans",
    summary = "Run the compliance scans now",
    description = "Runs the deterministic scan of the instance's database (rows past their retention, \
                   accounts archived longer than the `users` policy, data left on anonymized accounts; \
                   the free text of the messages is out of scope) and, when configured, of the Redis keys \
                   without TTL or with a TTL above `REDIS_LONG_TTL_SECONDS` (by prefix), writes each \
                   finding to the compliance journal and returns them: counts and locations, never a \
                   value. Service token only \
                   (HS256 JWT signed with the instance's `JWT_SECRET`, `role: service`).",
    responses(
        (status = 200, description = "Findings of the scan (possibly empty).", body = [Finding]),
        (status = 401, description = "Missing, invalid or non-service token.", body = String, content_type = "text/plain"),
        (status = 500, description = "The scan could not run.", body = String, content_type = "text/plain")
    ),
    tag = "Compliance"
)]
#[post("/scans")]
pub async fn run_scan(_caller: ServiceCaller, state: web::Data<AppState>) -> HttpResponse {
    let store = PgStore::new(state.get_smart_db().clone());
    match run_scans(&store).await {
        Ok(findings) => HttpResponse::Ok().json(findings),
        Err(error) => {
            tracing::error!(error = %error, "database compliance scan failed");
            HttpResponse::InternalServerError().body("The scan could not run.")
        }
    }
}
