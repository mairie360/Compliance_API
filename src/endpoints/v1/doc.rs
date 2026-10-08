use utoipa::OpenApi;

use super::{erasures, masking, scans};

#[derive(OpenApi)]
#[openapi(
    paths(scans::run_scan, erasures::start_erasure, erasures::read_erasure, masking::masking_patterns),
    components(schemas(crate::store::Finding, crate::store::StepState, crate::store::Step, crate::store::StepStatus, crate::masking::MaskingPattern)),
    tags((name = "Compliance", description = "Per-instance compliance service (MAIR-498): scan, erasure, journal"))
)]
pub struct V1Doc;
