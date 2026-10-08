use mairie360_api_lib::database::db_interface::{ApiRequestDto, QueryParam};
use serde::Deserialize;

/// `SELECT 1`: the cheapest round-trip proving Postgres accepts queries (readiness probe).
#[derive(Debug, Default, Deserialize)]
pub struct PingQueryView;

impl ApiRequestDto for PingQueryView {
    fn query_sql(&self) -> &'static str {
        "SELECT 1"
    }

    fn query_params(&self) -> &[QueryParam] {
        &[]
    }
}
