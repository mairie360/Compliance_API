pub mod db_error;
pub mod health;
pub mod pagination;
pub mod swagger;
pub mod v1;
pub mod validation;

use actix_web::web;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.configure(v1::config);
}
