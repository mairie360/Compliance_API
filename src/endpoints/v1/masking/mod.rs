//! `GET /api/v1/masking-patterns` (MAIR-498): the patterns the instance's OpenTelemetry Collector
//! applies to every log line before storage (`masking-patterns.yaml`). Service token only.

use crate::auth::ServiceCaller;
use crate::masking::{MaskingPattern, PATTERNS};
use actix_web::{get, HttpResponse};

#[utoipa::path(
    get,
    path = "/masking-patterns",
    summary = "Masking patterns of the logs",
    description = "The patterns (RE2 syntax, with their replacement) of the personal data a log line \
                   must never carry: e-mail, French phone number, JWT, bearer token, password hash. The \
                   collector replaces the matches before storage. IP addresses are kept for security by \
                   decision. Service token only.",
    responses(
        (status = 200, description = "The masking patterns.", body = [MaskingPattern]),
        (status = 401, description = "Missing, invalid or non-service token.", body = String, content_type = "text/plain")
    ),
    tag = "Compliance"
)]
#[get("/masking-patterns")]
pub async fn masking_patterns(_caller: ServiceCaller) -> HttpResponse {
    HttpResponse::Ok().json(&*PATTERNS)
}
