use utoipa::OpenApi;

use super::scans;

#[derive(OpenApi)]
#[openapi(
    paths(scans::run_scan),
    components(schemas(crate::store::Finding)),
    tags((name = "Compliance", description = "Per-instance compliance service (MAIR-498): scan, erasure, journal"))
)]
pub struct V1Doc;
