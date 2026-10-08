use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};

/// Builds the Postgres connection URL from its components.
///
/// The user, password and database name are percent-encoded, so any secret works,
/// including one containing `/`, `@`, `:`, `#`, `%` or a space. sqlx decodes them
/// back when it parses the URL. The host and port are used as-is.
pub fn build_pg_url(user: &str, password: &str, host: &str, port: &str, name: &str) -> String {
    format!(
        "postgres://{}:{}@{}:{}/{}",
        utf8_percent_encode(user, NON_ALPHANUMERIC),
        utf8_percent_encode(password, NON_ALPHANUMERIC),
        host,
        port,
        utf8_percent_encode(name, NON_ALPHANUMERIC)
    )
}
