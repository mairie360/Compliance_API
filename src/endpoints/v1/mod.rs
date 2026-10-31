pub mod doc;
pub mod erasures;
pub mod scans;

use actix_web::web;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/v1")
            .service(scans::run_scan)
            .service(erasures::start_erasure)
            .service(erasures::read_erasure),
    );
}
