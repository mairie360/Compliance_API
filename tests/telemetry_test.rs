//! OpenTelemetry tracing (MAIR-503, `mairie360_api_lib::logging::telemetry`): each request of
//! this API yields a root span carrying the route and the status, continues an incoming
//! `traceparent`, carries the SQL it ran as span events, and leaves without the client address
//! nor the query string. The redaction of the logs and spans themselves is tested in the lib.

use actix_web::test::TestRequest;
use actix_web::{web, App};
use compliance_api::endpoints::health;
use mairie360_api_lib::logging::telemetry::opentelemetry::{global, Value};
use mairie360_api_lib::logging::telemetry::opentelemetry_sdk::propagation::TraceContextPropagator;
use mairie360_api_lib::logging::telemetry::opentelemetry_sdk::trace::{
    InMemorySpanExporter, SdkTracerProvider, SimpleSpanProcessor, SpanData,
};
use mairie360_api_lib::logging::telemetry::{trace_layer, tracer_provider, TracingLogger};
use mairie360_api_lib::security::JwtMiddleware;
use mairie360_api_lib::state::AppState;
use mairie360_api_lib::test_setup::queries_setup::get_shared_db;
use serial_test::serial;
use tracing::subscriber::DefaultGuard;
use tracing_subscriber::layer::SubscriberExt;

/// Nothing listens on port 1: the readiness probe reports Redis down right away.
const UNREACHABLE_REDIS: &str = "redis://127.0.0.1:1";

/// Provider exporting to memory through the redaction of the lib, and the subscriber feeding
/// it, installed for the current thread.
fn in_memory_tracing() -> (InMemorySpanExporter, SdkTracerProvider, DefaultGuard) {
    global::set_text_map_propagator(TraceContextPropagator::new());
    let exporter = InMemorySpanExporter::default();
    let provider = tracer_provider(SimpleSpanProcessor::new(exporter.clone()), "compliance-api");
    let guard = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(trace_layer(&provider)),
    );
    (exporter, provider, guard)
}

fn attribute<'a>(span: &'a SpanData, key: &str) -> Option<&'a Value> {
    span.attributes
        .iter()
        .find(|kv| kv.key.as_str() == key)
        .map(|kv| &kv.value)
}

/// The readiness probe runs `SELECT 1` through the lib: its span carries that statement.
#[actix_web::test]
#[serial]
async fn a_request_is_exported_as_a_span_with_its_sql() {
    let (exporter, provider, _guard) = in_memory_tracing();
    let (_container, pg_url) = get_shared_db().await;
    let state = AppState::new(UNREACHABLE_REDIS.to_string(), pg_url.clone()).await;
    let app = actix_web::test::init_service(
        App::new()
            .wrap(TracingLogger::default())
            .app_data(web::Data::new(state))
            .service(health::ready),
    )
    .await;
    exporter.reset();

    let trace_id = "4bf92f3577b34da6a3ce929d0e0e4736";
    let request = TestRequest::get()
        .uri("/ready?probe=kubelet")
        .insert_header(("traceparent", format!("00-{trace_id}-00f067aa0ba902b7-01")))
        .to_request();
    let response = actix_web::test::call_service(&app, request).await;
    assert_eq!(response.status().as_u16(), 503, "Redis is unreachable");
    // The root span closes with the response body: drop it before reading the exported spans.
    drop(response);

    provider.force_flush().expect("flush the spans");
    let spans = exporter.get_finished_spans().unwrap();
    let span = spans
        .iter()
        .find(|span| span.name == "GET /ready")
        .unwrap_or_else(|| panic!("no span named GET /ready: {spans:#?}"));
    assert_eq!(
        span.span_context.trace_id().to_string(),
        trace_id,
        "the incoming traceparent must be continued"
    );
    assert_eq!(attribute(span, "http.status_code"), Some(&Value::I64(503)));
    assert_eq!(attribute(span, "http.route"), Some(&Value::from("/ready")));
    assert_eq!(
        attribute(span, "http.target"),
        Some(&Value::from("/ready")),
        "the query string must not be exported"
    );
    assert_eq!(attribute(span, "http.client_ip"), None);

    // `sqlx` logs each statement as a `sqlx::query` event; the values stay bound parameters.
    let has_sql = span.events.iter().any(|event| {
        let has = |key: &str| event.attributes.iter().any(|kv| kv.key.as_str() == key);
        has("db.statement")
            && event
                .attributes
                .iter()
                .any(|kv| kv.key.as_str() == "target" && kv.value == Value::from("sqlx::query"))
    });
    assert!(
        has_sql,
        "the SQL run by the handler must be attached to its span: {:#?}",
        span.events
    );
}

/// Mounted like `main.rs` (`TracingLogger` around the `/api` scope and its `JwtMiddleware`): a
/// request the gate refuses before any handler runs still gets its span, with its status.
#[actix_web::test]
#[serial]
async fn a_refused_request_still_gets_its_span() {
    let (exporter, provider, _guard) = in_memory_tracing();
    let (_container, pg_url) = get_shared_db().await;
    let state = AppState::new(UNREACHABLE_REDIS.to_string(), pg_url.clone()).await;
    let app = actix_web::test::init_service(
        App::new()
            .wrap(TracingLogger::default())
            .app_data(web::Data::new(state))
            .service(
                web::scope("/api")
                    .wrap(JwtMiddleware)
                    .configure(compliance_api::endpoints::config),
            ),
    )
    .await;
    exporter.reset();

    // No JWT: `JwtMiddleware` refuses before the handler runs.
    let _ = actix_web::test::try_call_service(
        &app,
        TestRequest::get().uri("/api/v1/scans/").to_request(),
    )
    .await;

    provider.force_flush().expect("flush the spans");
    let spans = exporter.get_finished_spans().unwrap();
    let span = spans
        .iter()
        .find(|span| span.name.starts_with("GET"))
        .unwrap_or_else(|| panic!("no span for the refused request: {spans:#?}"));
    assert_eq!(attribute(span, "http.status_code"), Some(&Value::I64(401)));
}
