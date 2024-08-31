# OpenTelemetry with [Tracing](https://crates.io/crates/tracing) instrumentation

This example shows how to use the tracing framework with opentelemetry.

## Usage

Launch collector and example:

```shell

# Install otelcol binary https://opentelemetry.io/docs/collector/installation/

# Run the opentelemetry collector
# terminal 1
otelcol --config config.yml

# Run the app
# terminal 2
cargo run
# cargo run -- --otel .env-my-otel

```
