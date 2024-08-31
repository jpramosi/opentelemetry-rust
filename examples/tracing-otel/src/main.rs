use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::{global, KeyValue};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_sdk::logs::LoggerProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::resource::{EnvResourceDetector, ResourceDetector};
use opentelemetry_sdk::trace::{self, TracerProvider};
use opentelemetry_sdk::{
    metrics::{
        reader::{DefaultAggregationSelector, DefaultTemporalitySelector},
        MeterProviderBuilder, PeriodicReader, SdkMeterProvider,
    },
    runtime,
    trace::{RandomIdGenerator, Sampler},
    Resource,
};
use opentelemetry_semantic_conventions::{
    resource::{DEPLOYMENT_ENVIRONMENT_NAME, SERVICE_NAME, SERVICE_VERSION},
    SCHEMA_URL,
};
use opentelemetry_tracing::{MetricsLayer, OpenTelemetryLayer};
use tracing::*;
use tracing_core::Level;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

// Create a Resource that captures information about the entity for which telemetry is recorded.
fn resource() -> Resource {
    Resource::from_schema_url(
        [
            KeyValue::new(SERVICE_NAME, env!("CARGO_PKG_NAME")),
            KeyValue::new(SERVICE_VERSION, env!("CARGO_PKG_VERSION")),
            KeyValue::new(DEPLOYMENT_ENVIRONMENT_NAME, "develop"),
        ],
        SCHEMA_URL,
    )
}

pub struct OtelGuard {
    pub tracer_provider: TracerProvider,
    pub meter_provider: SdkMeterProvider,
    pub logger_provider: LoggerProvider,
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        for (i, r) in self.logger_provider.force_flush().iter().enumerate() {
            if let Err(err) = r {
                eprintln!("Failed to flush log message {i}: {err:?}");
            }
        }
        if let Err(err) = self.meter_provider.force_flush() {
            eprintln!("Failed to flush metric messages: {err:?}");
        }
        for (i, r) in self.tracer_provider.force_flush().iter().enumerate() {
            if let Err(err) = r {
                eprintln!("Failed to flush trace message {i}: {err:?}");
            }
        }
    }
}

#[derive(Clone, Debug, Parser)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    /// The opentelemetry environment file.
    #[clap(long, default_value = ".env")]
    pub otel: PathBuf,

    /// The opentelemetry protocol to use. <grpc|http>
    #[clap(long, default_value = "grpc")]
    pub proto: String,
}

fn init_tracing_grpc() -> OtelGuard {
    let meter_provider = MeterProviderBuilder::default()
        .with_resource(resource())
        .with_reader(
            PeriodicReader::builder(
                opentelemetry_otlp::new_exporter()
                    .tonic()
                    .build_metrics_exporter(
                        Box::new(DefaultAggregationSelector::new()),
                        Box::new(DefaultTemporalitySelector::new()),
                    )
                    .unwrap(),
                runtime::Tokio,
            )
            .with_interval(std::time::Duration::from_secs(30))
            .build(),
        )
        .with_reader(
            PeriodicReader::builder(
                opentelemetry_stdout::MetricsExporter::default(),
                runtime::Tokio,
            )
            .build(),
        )
        .build();

    global::set_meter_provider(meter_provider.clone());

    let tracer_provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(opentelemetry_otlp::new_exporter().tonic())
        .with_trace_config(
            trace::Config::default()
                .with_sampler(Sampler::AlwaysOn)
                .with_id_generator(RandomIdGenerator::default())
                .with_max_events_per_span(64)
                .with_max_attributes_per_span(16)
                .with_max_events_per_span(16)
                .with_resource(EnvResourceDetector::new().detect(Duration::from_secs(5))),
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio)
        .unwrap();

    global::set_tracer_provider(tracer_provider.clone());

    let logger_provider = opentelemetry_otlp::new_pipeline()
        .logging()
        .with_exporter(opentelemetry_otlp::new_exporter().tonic())
        .install_batch(opentelemetry_sdk::runtime::Tokio)
        .unwrap();

    global::set_text_map_propagator(TraceContextPropagator::new());

    tracing_subscriber::registry()
        .with(tracing_subscriber::filter::LevelFilter::from_level(
            Level::INFO,
        ))
        .with(tracing_subscriber::fmt::layer())
        .with(MetricsLayer::new(meter_provider.clone()))
        .with(OpenTelemetryLayer::new(
            tracer_provider.tracer(env!("CARGO_PKG_NAME")),
        ))
        .with(OpenTelemetryTracingBridge::new(&logger_provider))
        .init();

    OtelGuard {
        tracer_provider,
        meter_provider,
        logger_provider,
    }
}

fn init_tracing_http() -> OtelGuard {
    let meter_provider = MeterProviderBuilder::default()
        .with_resource(resource())
        .with_reader(
            PeriodicReader::builder(
                opentelemetry_otlp::new_exporter()
                    .http()
                    .with_http_client(
                        reqwest::Client::builder()
                            .danger_accept_invalid_certs(true)
                            .build()
                            .unwrap(),
                    )
                    .build_metrics_exporter(
                        Box::new(DefaultAggregationSelector::new()),
                        Box::new(DefaultTemporalitySelector::new()),
                    )
                    .unwrap(),
                runtime::Tokio,
            )
            .with_interval(std::time::Duration::from_secs(30))
            .build(),
        )
        .with_reader(
            PeriodicReader::builder(
                opentelemetry_stdout::MetricsExporter::default(),
                runtime::Tokio,
            )
            .build(),
        )
        .build();

    global::set_meter_provider(meter_provider.clone());

    let tracer_provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter().http().with_http_client(
                reqwest::Client::builder()
                    .danger_accept_invalid_certs(true)
                    .build()
                    .unwrap(),
            ),
        )
        .with_trace_config(
            trace::Config::default()
                .with_sampler(Sampler::AlwaysOn)
                .with_id_generator(RandomIdGenerator::default())
                .with_max_events_per_span(64)
                .with_max_attributes_per_span(16)
                .with_max_events_per_span(16)
                .with_resource(EnvResourceDetector::new().detect(Duration::from_secs(5))),
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio)
        .unwrap();

    global::set_tracer_provider(tracer_provider.clone());

    let logger_provider = opentelemetry_otlp::new_pipeline()
        .logging()
        .with_exporter(
            opentelemetry_otlp::new_exporter().http().with_http_client(
                reqwest::Client::builder()
                    .danger_accept_invalid_certs(true)
                    .build()
                    .unwrap(),
            ),
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio)
        .unwrap();

    global::set_text_map_propagator(TraceContextPropagator::new());

    tracing_subscriber::registry()
        .with(tracing_subscriber::filter::LevelFilter::from_level(
            Level::INFO,
        ))
        .with(tracing_subscriber::fmt::layer())
        .with(MetricsLayer::new(meter_provider.clone()))
        .with(OpenTelemetryLayer::new(
            tracer_provider.tracer(env!("CARGO_PKG_NAME")),
        ))
        .with(OpenTelemetryTracingBridge::new(&logger_provider))
        .init();

    OtelGuard {
        tracer_provider,
        meter_provider,
        logger_provider,
    }
}

#[instrument]
async fn cut_my_apple(slices: usize) -> Result<(), ()> {
    if slices <= 0 {
        error!(target: "vendor", "I cannot cut the apple in {slices} slices");
        return Err(());
    }
    info!(target: "vendor", "I cut the apple in {slices} slices");
    return Ok(());
}

// Emit span using macros from the tracing crate.
#[instrument]
async fn my_instumented_fun() {
    // Emit logs using macros from the tracing crate.
    // These logs gets piped through OpenTelemetry bridge.
    info!(target: "my-target", "hello from {}. My price is {}", "apple vendor", 2.99);
    cut_my_apple(0)
        .await
        .unwrap_or(cut_my_apple(4).await.unwrap());
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();

    dotenvy::from_path(&args.otel).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to load environment file {:?}: {}", args.otel, err),
        )
    })?;

    let _guard = if args.proto == "grpc" {
        init_tracing_grpc()
    } else if args.proto == "http" {
        init_tracing_http()
    } else {
        panic!("OpenTelemetry protocol '{}' not supported", args.proto);
    };

    my_instumented_fun().await;

    return Ok(());
}
