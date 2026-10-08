//! OpenTelemetry tracing (MAIR-503): each request yields a root span carrying the route and the
//! status, continues an incoming `traceparent`, carries the SQL it ran as span events, and leaves
//! without the client address nor the query string, in the spans as in the logs.

use std::io::Write;
use std::sync::{Arc, Mutex};

use actix_web::test::TestRequest;
use actix_web::{middleware, web, App, HttpResponse};
use compliance_api::endpoints::health;
use compliance_api::telemetry::{log_layer, trace_layer, tracer_provider};
use mairie360_api_lib::state::AppState;
use mairie360_api_lib::test_setup::queries_setup::get_shared_db;
use opentelemetry::global;
use opentelemetry::Value;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{
    InMemorySpanExporter, SdkTracerProvider, SimpleSpanProcessor, SpanData,
};
use serial_test::serial;
use tracing::subscriber::DefaultGuard;
use tracing_actix_web::TracingLogger;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;

/// Nothing listens on port 1: the readiness probe reports Redis down right away.
const UNREACHABLE_REDIS: &str = "redis://127.0.0.1:1";

/// Provider exporting to memory through the redaction of `telemetry`, and the subscriber feeding
/// it, installed for the current thread.
fn in_memory_tracing() -> (InMemorySpanExporter, SdkTracerProvider, DefaultGuard) {
    global::set_text_map_propagator(TraceContextPropagator::new());
    let exporter = InMemorySpanExporter::default();
    let provider = tracer_provider(SimpleSpanProcessor::new(exporter.clone()));
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

/// Collects the logs of [`log_layer`].
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'writer> MakeWriter<'writer> for Captured {
    type Writer = Self;

    fn make_writer(&'writer self) -> Self::Writer {
        self.clone()
    }
}

#[actix_web::get("/api/v1/scans")]
async fn logging_handler() -> HttpResponse {
    tracing::error!("the handler logs an error");
    HttpResponse::Ok().finish()
}

/// No database: the logs and spans of a request carrying personal data in its query string and
/// client address, mounted like `main.rs` (`Logger`, then `TracingLogger`).
#[actix_web::test]
#[serial]
async fn neither_logs_nor_spans_carry_the_query_string_or_the_client_address() {
    global::set_text_map_propagator(TraceContextPropagator::new());
    let exporter = InMemorySpanExporter::default();
    let provider = tracer_provider(SimpleSpanProcessor::new(exporter.clone()));
    let logs = Captured::default();
    let _guard = tracing::subscriber::set_default(
        tracing_subscriber::registry()
            .with(log_layer(logs.clone()))
            .with(trace_layer(&provider)),
    );
    let app = actix_web::test::init_service(
        App::new()
            .wrap(middleware::Logger::default())
            .wrap(TracingLogger::default())
            .service(logging_handler),
    )
    .await;

    let request = TestRequest::get()
        .uri("/api/v1/scans?search=Dupont")
        .insert_header(("x-forwarded-for", "203.0.113.7"))
        .to_request();
    drop(actix_web::test::call_service(&app, request).await);
    provider.force_flush().expect("flush the spans");

    let logs = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    let event = logs
        .lines()
        .find(|line| line.contains("the handler logs an error"))
        .unwrap_or_else(|| panic!("the handler's event must be logged: {logs}"));
    assert!(
        !event.contains("Dupont"),
        "query string in the logs: {event}"
    );
    assert!(
        !event.contains("203.0.113.7"),
        "client address in the logs: {event}"
    );

    let spans = format!("{:?}", exporter.get_finished_spans().unwrap());
    assert!(spans.contains("the handler logs an error"), "{spans}");
    assert!(
        !spans.contains("Dupont"),
        "query string in the spans: {spans}"
    );
    assert!(
        !spans.contains("203.0.113.7"),
        "client address in the spans: {spans}"
    );
}
