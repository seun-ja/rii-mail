use opentelemetry::{global, trace::TracerProvider as _};
use opentelemetry_otlp::{SpanExporter, WithExportConfig as _};
use opentelemetry_sdk::{
    trace::{SdkTracerProvider, Tracer},
    Resource,
};

use tracing::subscriber::set_global_default;

use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt as _,
    EnvFilter, Registry,
};

/// Initializes the subscriber with the given log levels.
pub fn init_subscriber(
    log_level: &str,
    otlp_collector_endpoint: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let env_filter = EnvFilter::new(log_level);

    let fmt_layer = fmt::layer()
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        .pretty();

    let telemetry_opt_in = std::env::var("TELEMETRY_OPT_IN")
        .ok()
        .and_then(|value| value.parse::<bool>().ok())
        .unwrap_or(false);

    let telemetry_layer = telemetry_opt_in
        .then(|| init_tracer(otlp_collector_endpoint))
        .transpose()?
        .map(|tracer| tracing_opentelemetry::layer().with_tracer(tracer));

    let subscriber = Registry::default().with(env_filter).with(fmt_layer);

    // If subscriber is already set, this will fail - that's OK, we just ignore it
    // This allows init_subscriber to be called multiple times safely
    if let Some(telemetry_layer) = telemetry_layer {
        set_global_default(subscriber.with(telemetry_layer))?;
    } else {
        set_global_default(subscriber)?;
    }

    Ok(())
}

pub fn init_tracer(
    otlp_collector_endpoint: &str,
) -> Result<Tracer, Box<dyn std::error::Error + Send + Sync>> {
    let exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(otlp_collector_endpoint)
        .build()?;

    let resource = Resource::builder()
        .with_service_name("add-on-client")
        .build();

    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    global::set_tracer_provider(provider.clone());

    Ok(provider.tracer("add-on-client"))
}
