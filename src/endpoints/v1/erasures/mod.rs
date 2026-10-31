//! `POST` / `GET /api/v1/erasures/{userId}` (MAIR-498): requests (or resumes) and reads the
//! erasure of a user. Service token only: Core_API calls it when an administrator erases an
//! account.

use crate::auth::ServiceCaller;
use crate::erasure::connectors::Connectors;
use crate::erasure::run_erasure;
use crate::store::pg::PgStore;
use crate::store::{ComplianceStore, StepState};
use actix_web::{get, post, web, HttpResponse};
use mairie360_api_lib::state::AppState;

fn store(state: &AppState) -> PgStore {
    PgStore::new(state.get_smart_db().clone())
}

#[utoipa::path(
    post,
    path = "/erasures/{userId}",
    summary = "Erase a user everywhere",
    description = "Starts (or resumes) the erasure of the user: Keycloak account and Resend contact \
                   (with the e-mail and Keycloak subject read first), then the database \
                   (`anonymize_user`), then the S3 objects, the Redis keys and the backup key \
                   (MAIR-500). Each step is idempotent, recorded in `erasure_steps`, journaled as \
                   proof without the data, and retried in the background until done. A service \
                   that is not configured on the instance is recorded as such. Service token only.",
    params(("userId" = i32, Path, description = "Id of the account to erase.")),
    responses(
        (status = 202, description = "Erasure started or resumed: the steps and their status.", body = [StepState]),
        (status = 401, description = "Missing, invalid or non-service token.", body = String, content_type = "text/plain"),
        (status = 404, description = "No such account.", body = String, content_type = "text/plain"),
        (status = 500, description = "The erasure could not be recorded.", body = String, content_type = "text/plain")
    ),
    tag = "Compliance"
)]
#[post("/erasures/{userId}")]
pub async fn start_erasure(
    caller: ServiceCaller,
    state: web::Data<AppState>,
    connectors: web::Data<Connectors>,
    path: web::Path<i32>,
) -> HttpResponse {
    let user_id = path.into_inner();
    let store = store(&state);
    match store.user_exists(user_id).await {
        Ok(true) => {}
        Ok(false) => return HttpResponse::NotFound().body("No such account."),
        Err(error) => {
            tracing::error!(error = %error, "erasure: cannot check the account");
            return HttpResponse::InternalServerError().body("The erasure could not be recorded.");
        }
    }
    tracing::info!(user_id, caller = %caller.0, "erasure requested");
    match run_erasure(&store, &connectors, user_id).await {
        Ok(steps) => HttpResponse::Accepted().json(steps),
        Err(error) => {
            tracing::error!(user_id, error = %error, "erasure could not be recorded");
            HttpResponse::InternalServerError().body("The erasure could not be recorded.")
        }
    }
}

#[utoipa::path(
    get,
    path = "/erasures/{userId}",
    summary = "Read the erasure of a user",
    description = "The steps of the erasure of the user and their status (`last_error` never holds a \
                   value of the user). Service token only.",
    params(("userId" = i32, Path, description = "Id of the account.")),
    responses(
        (status = 200, description = "The steps of the erasure.", body = [StepState]),
        (status = 401, description = "Missing, invalid or non-service token.", body = String, content_type = "text/plain"),
        (status = 404, description = "No erasure was requested for this account.", body = String, content_type = "text/plain"),
        (status = 500, description = "The steps could not be read.", body = String, content_type = "text/plain")
    ),
    tag = "Compliance"
)]
#[get("/erasures/{userId}")]
pub async fn read_erasure(
    _caller: ServiceCaller,
    state: web::Data<AppState>,
    path: web::Path<i32>,
) -> HttpResponse {
    match store(&state).steps(path.into_inner()).await {
        Ok(steps) if steps.is_empty() => {
            HttpResponse::NotFound().body("No erasure was requested for this account.")
        }
        Ok(steps) => HttpResponse::Ok().json(steps),
        Err(error) => {
            tracing::error!(error = %error, "erasure: cannot read the steps");
            HttpResponse::InternalServerError().body("The steps could not be read.")
        }
    }
}
