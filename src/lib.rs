// An id read as `u64` and cast with `as i32` silently wraps (`2^32 + 1` becomes `1`, another row):
// convert with `mairie360_api_lib::database::db_interface::id_to_sql` / `id_from_sql` or
// `i32::try_from` instead (MAIR-422). Keep these lints on in every API generated from the template.
#![deny(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

pub mod auth;
pub mod backup;
pub mod database;
pub mod endpoints;
pub mod erasure;
pub mod masking;
pub mod scan;
pub mod store;
