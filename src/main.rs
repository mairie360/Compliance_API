// An id read as `u64` and cast with `as i32` silently wraps (`2^32 + 1` becomes `1`, another row):
// convert with `mairie360_api_lib::database::db_interface::id_to_sql` / `id_from_sql` or
// `i32::try_from` instead (MAIR-422). Keep these lints on in every API generated from the template.
#![deny(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

use actix_web::{middleware, web, App, HttpServer};

use compliance_api::database::pg_url::build_pg_url;
use compliance_api::endpoints::health::wait_for_postgres;
use compliance_api::endpoints::swagger::{api_docs_enabled, ApiDoc, API_DOCS_ENABLED};
use compliance_api::endpoints::{config, health};

use mairie360_api_lib::env_manager::{get_critical_env_var, get_env_var};
use mairie360_api_lib::security::JwtMiddleware;
use mairie360_api_lib::state::AppState;

use tracing_subscriber::EnvFilter;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

/// Postgres gets this many tries at startup, [`STARTUP_DB_RETRY_DELAY`] apart (about 30 s in all with
/// the 2 s timeout of each try), before the API gives up.
const STARTUP_DB_ATTEMPTS: u32 = 10;
const STARTUP_DB_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(1);

//                                        -- MAIN FUNCTION --

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Without a subscriber, actix's `Logger` and every `tracing` event are silently dropped.
    // `RUST_LOG` overrides the level; database errors are logged at `error`.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let redis_url = get_critical_env_var("REDIS_URL");
    let db_user = get_critical_env_var("DB_USER");
    let db_password = get_critical_env_var("DB_PASSWORD");
    let db_host = get_critical_env_var("DB_HOST");
    let db_port = get_critical_env_var("DB_PORT");
    let db_name = get_critical_env_var("DB_NAME");
    let pg_url = build_pg_url(&db_user, &db_password, &db_host, &db_port, &db_name);
    let state = AppState::new(redis_url, pg_url).await;
    // The lib keeps going without a pool when Postgres is unreachable: refuse to start instead, so
    // the pod restarts (with backoff) rather than staying up and answering 500 to every request.
    if !wait_for_postgres(&state, STARTUP_DB_ATTEMPTS, STARTUP_DB_RETRY_DELAY).await {
        tracing::error!("Postgres did not answer at startup, exiting");
        return Err(std::io::Error::other("Postgres unreachable at startup"));
    }
    let data = web::Data::new(state);
    let host = get_critical_env_var("HOST");
    let port = get_critical_env_var("PORT");
    let bind_address = format!("{}:{}", host, port);
    let docs_enabled = api_docs_enabled(get_env_var(API_DOCS_ENABLED).as_deref());
    tracing::info!(docs_enabled, "Swagger UI and OpenAPI document served");

    let server = HttpServer::new(move || {
        App::new()
            .app_data(data.clone())
            .wrap(middleware::Logger::default())
            // Every response is JSON or plain text: forbid browsers from sniffing it as HTML.
            .wrap(middleware::DefaultHeaders::new().add(("X-Content-Type-Options", "nosniff")))
            // 1. Swagger UI and the OpenAPI document (public), only where explicitly enabled:
            //    dev and test stacks, never the production deployment.
            .configure(|cfg| {
                if docs_enabled {
                    cfg.service(
                        SwaggerUi::new("/swagger-ui/{_:.*}")
                            .url("/api-docs/openapi.json", ApiDoc::openapi()),
                    );
                }
            })
            // 2. Public probes
            .service(health::health)
            .service(health::ready)
            // 3. Endpoints protected by the JWT
            .service(web::scope("/api").wrap(JwtMiddleware).configure(config))
    })
    .bind(bind_address)?;

    let addr = server.addrs().first().copied();
    tokio::spawn(async move {
        if let Some(addr) = addr {
            tracing::info!("Server listening on http://{addr}");
        }
    });

    server.run().await
}
