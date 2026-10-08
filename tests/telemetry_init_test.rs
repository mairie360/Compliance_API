//! `telemetry::init` installs the process-wide subscriber, so it is tested in its own binary.

use compliance_api::telemetry;

#[test]
fn init_exports_when_an_endpoint_is_set_and_installs_once() {
    // Nothing listens on port 1: the spans are dropped at the export, the API keeps running.
    std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://127.0.0.1:1");
    let guard = telemetry::init();
    assert!(guard.is_some(), "an endpoint turns the export on");
    tracing::info_span!("request").in_scope(|| tracing::info!("exported nowhere"));

    assert!(
        telemetry::init().is_none(),
        "a second init finds the subscriber installed and exports nothing"
    );
    // Flushes and shuts the provider down; the failed export is only printed.
    drop(guard);
}
