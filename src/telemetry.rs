//! OpenTelemetry tracing (MAIR-503, same approach as the Core API POC of MAIR-131).
//!
//! Traces are exported over OTLP/HTTP (protobuf) to a collector or agent (`OTel` Collector, Grafana
//! Alloy) that relays them to the observability backend (Scaleway Cockpit). Everything is opt-in
//! and driven by the standard OpenTelemetry environment variables, so a deployment without a
//! collector behaves exactly as before:
//!
//! - `OTEL_EXPORTER_OTLP_ENDPOINT` (or `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`): enables the export.
//!   With the generic variable, `/v1/traces` is appended, e.g. `http://alloy:4318`.
//! - `OTEL_SDK_DISABLED=true`: forces the export off even when an endpoint is set.
//! - `OTEL_SERVICE_NAME` (default `compliance-api`), `OTEL_RESOURCE_ATTRIBUTES`,
//!   `OTEL_EXPORTER_OTLP_HEADERS`, `OTEL_EXPORTER_OTLP_TIMEOUT`, `OTEL_TRACES_SAMPLER`: read by the
//!   SDK itself.
//! - `RUST_LOG`: filter of the stdout logs (default `info`), on whether the export is or not.
//!
//! Each HTTP request gets a root span from `tracing_actix_web::TracingLogger` (it continues the
//! `traceparent` sent by a BFF). The SQL statements run by `mairie360_api_lib` through `sqlx` are
//! attached to it as span events (`db.statement`, `elapsed`, rows) because `sqlx` reports them as
//! `tracing` events of the `sqlx::query` target, not as spans of their own.
//!
//! Spans never leave the process with personal data (MAIR-290, MAIR-501): [`Redact`] drops the
//! client address and the query string of `http.target` before the export.

use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::{Context, Key, KeyValue, Value};
use opentelemetry_otlp::{Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{
    BatchSpanProcessor, SdkTracerProvider, Span, SpanData, SpanProcessor,
};
use opentelemetry_sdk::Resource;
use std::time::Duration;
use tracing::Subscriber;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

/// `service.name` reported when `OTEL_SERVICE_NAME` is not set.
pub const DEFAULT_SERVICE_NAME: &str = "compliance-api";

/// Filter of the trace layer: `sqlx::query` is a `DEBUG` target, everything else stays at `INFO`,
/// except actix's request log (request line and client address), kept out of the spans.
const TRACE_FILTER: &str = "info,sqlx::query=debug,actix_web::middleware::logger=off";

/// Hides the root span of `tracing_actix_web` from the stdout logs: the fmt layer prints the
/// fields of the enclosing spans in front of each event, and that span holds the client address
/// and the query string (MAIR-290).
const ROOT_SPAN_LOG_DIRECTIVE: &str = "tracing_actix_web=off";

/// Instrumentation scope name of the tracer.
const TRACER_NAME: &str = "compliance_api";

/// Root span attributes that identify a person: dropped before the export.
const DROPPED_ATTRIBUTES: [&str; 1] = ["http.client_ip"];

/// Root span attribute holding the path and query string: only the path is exported.
const TARGET_ATTRIBUTE: &str = "http.target";

/// Keeps the tracer provider alive: dropping it flushes the spans still buffered.
pub struct TelemetryGuard {
    provider: SdkTracerProvider,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Err(error) = self.provider.shutdown() {
            eprintln!("OpenTelemetry shutdown failed: {error}");
        }
    }
}

/// Tells whether the environment asks for the trace export, given a variable lookup.
#[must_use]
pub fn is_enabled(lookup: impl Fn(&str) -> Option<String>) -> bool {
    let set = |name: &str| lookup(name).is_some_and(|value| !value.trim().is_empty());
    let disabled =
        lookup("OTEL_SDK_DISABLED").is_some_and(|value| value.trim().eq_ignore_ascii_case("true"));
    !disabled && (set("OTEL_EXPORTER_OTLP_ENDPOINT") || set("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT"))
}

/// Resource describing this service: `service.name` (`OTEL_SERVICE_NAME` wins over
/// [`DEFAULT_SERVICE_NAME`]) plus whatever `OTEL_RESOURCE_ATTRIBUTES` declares.
fn resource() -> Resource {
    let builder = Resource::builder();
    if std::env::var_os("OTEL_SERVICE_NAME").is_some() {
        builder.build()
    } else {
        builder.with_service_name(DEFAULT_SERVICE_NAME).build()
    }
}

/// Span processor stripping the personal data of the spans before handing them to `inner`.
#[derive(Debug)]
pub struct Redact<P>(pub P);

/// Removes the attributes of [`DROPPED_ATTRIBUTES`] and the query string of `http.target`.
fn redact(span: &mut SpanData) {
    span.attributes
        .retain(|kv| !DROPPED_ATTRIBUTES.contains(&kv.key.as_str()));
    for kv in &mut span.attributes {
        if kv.key.as_str() == TARGET_ATTRIBUTE {
            let target = kv.value.as_str();
            if let Some((path, _query)) = target.split_once('?') {
                *kv = KeyValue::new(
                    Key::from_static_str(TARGET_ATTRIBUTE),
                    Value::from(path.to_owned()),
                );
            }
        }
    }
}

impl<P: SpanProcessor> SpanProcessor for Redact<P> {
    fn on_start(&self, span: &mut Span, cx: &Context) {
        self.0.on_start(span, cx);
    }

    fn on_end(&self, mut span: SpanData) {
        redact(&mut span);
        self.0.on_end(span);
    }

    fn force_flush(&self) -> OTelSdkResult {
        self.0.force_flush()
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        self.0.shutdown_with_timeout(timeout)
    }

    fn set_resource(&mut self, resource: &Resource) {
        self.0.set_resource(resource);
    }
}

/// Tracer provider of this service: its resource, and `processor` behind [`Redact`].
#[must_use]
pub fn tracer_provider(processor: impl SpanProcessor + 'static) -> SdkTracerProvider {
    SdkTracerProvider::builder()
        .with_resource(resource())
        .with_span_processor(Redact(processor))
        .build()
}

/// Stdout log layer writing to `writer`: filtered by `RUST_LOG` (default `info`), without the
/// request span's fields.
#[must_use]
pub fn log_layer<S, W>(writer: W) -> impl Layer<S>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"))
        .add_directive(
            ROOT_SPAN_LOG_DIRECTIVE
                .parse()
                .expect("ROOT_SPAN_LOG_DIRECTIVE is a valid directive"),
        );
    tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_filter(filter)
}

/// Trace layer bridging `tracing` spans to OpenTelemetry through `provider`.
#[must_use]
pub fn trace_layer<S>(provider: &SdkTracerProvider) -> impl Layer<S>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    tracing_opentelemetry::layer()
        .with_tracer(provider.tracer(TRACER_NAME))
        .with_filter(EnvFilter::new(TRACE_FILTER))
}

/// Starts the stdout logs, and the trace export when the environment asks for it. Returns the
/// guard to keep until exit (`None` without export).
///
/// Observability must never take the API down: when the exporter cannot be built, the reason is
/// logged and the API runs without traces.
#[must_use]
pub fn init() -> Option<TelemetryGuard> {
    let provider = if is_enabled(|name| std::env::var(name).ok()) {
        match SpanExporter::builder()
            .with_http()
            .with_protocol(Protocol::HttpBinary)
            .build()
        {
            Ok(exporter) => Some(tracer_provider(
                BatchSpanProcessor::builder(exporter).build(),
            )),
            Err(error) => {
                eprintln!("OpenTelemetry disabled: cannot build the OTLP exporter: {error}");
                None
            }
        }
    } else {
        None
    };

    let logs = log_layer(std::io::stdout);
    let traces = provider.as_ref().map(trace_layer);
    if let Err(error) = tracing_subscriber::registry()
        .with(logs)
        .with(traces)
        .try_init()
    {
        eprintln!("Logs and traces disabled: a tracing subscriber is already installed: {error}");
        return None;
    }
    let provider = provider?;
    global::set_tracer_provider(provider.clone());
    global::set_text_map_propagator(TraceContextPropagator::new());
    Some(TelemetryGuard { provider })
}

#[cfg(test)]
mod tests {
    use super::{is_enabled, redact};
    use opentelemetry::trace::{SpanContext, SpanKind, Status};
    use opentelemetry::{InstrumentationScope, KeyValue, Value};
    use opentelemetry_sdk::trace::{SpanData, SpanEvents, SpanLinks};
    use std::collections::HashMap;
    use std::time::SystemTime;

    fn lookup(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect();
        move |name| vars.get(name).cloned()
    }

    #[test]
    fn disabled_without_an_endpoint() {
        assert!(!is_enabled(lookup(&[])));
        assert!(!is_enabled(lookup(&[(
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            "  "
        )])));
    }

    #[test]
    fn enabled_by_the_generic_or_the_traces_endpoint() {
        assert!(is_enabled(lookup(&[(
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            "http://alloy:4318"
        )])));
        assert!(is_enabled(lookup(&[(
            "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
            "http://alloy:4318/v1/traces"
        )])));
    }

    #[test]
    fn sdk_disabled_wins_over_the_endpoint() {
        assert!(!is_enabled(lookup(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://alloy:4318"),
            ("OTEL_SDK_DISABLED", "TRUE"),
        ])));
        assert!(is_enabled(lookup(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://alloy:4318"),
            ("OTEL_SDK_DISABLED", "false"),
        ])));
    }

    fn span(attributes: Vec<KeyValue>) -> SpanData {
        SpanData {
            span_context: SpanContext::empty_context(),
            parent_span_id: opentelemetry::trace::SpanId::INVALID,
            parent_span_is_remote: false,
            span_kind: SpanKind::Server,
            name: "GET /api/v1/scans".into(),
            start_time: SystemTime::now(),
            end_time: SystemTime::now(),
            attributes,
            dropped_attributes_count: 0,
            events: SpanEvents::default(),
            links: SpanLinks::default(),
            status: Status::Unset,
            instrumentation_scope: InstrumentationScope::default(),
        }
    }

    #[test]
    fn redact_drops_the_client_address_and_the_query_string() {
        let mut data = span(vec![
            KeyValue::new("http.client_ip", "203.0.113.7"),
            KeyValue::new("http.target", "/api/v1/scans?search=Dupont"),
            KeyValue::new("http.route", "/api/v1/scans"),
        ]);
        redact(&mut data);
        let value = |key: &str| {
            data.attributes
                .iter()
                .find(|kv| kv.key.as_str() == key)
                .map(|kv| kv.value.clone())
        };
        assert_eq!(value("http.client_ip"), None);
        assert_eq!(value("http.target"), Some(Value::from("/api/v1/scans")));
        assert_eq!(value("http.route"), Some(Value::from("/api/v1/scans")));
    }

    #[test]
    fn redact_keeps_a_target_without_query_string() {
        let mut data = span(vec![KeyValue::new("http.target", "/api/v1/scans/3")]);
        redact(&mut data);
        assert_eq!(
            data.attributes,
            vec![KeyValue::new("http.target", "/api/v1/scans/3")]
        );
    }
}
