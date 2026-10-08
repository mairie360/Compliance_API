use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(nest())]
pub struct V1Doc;
