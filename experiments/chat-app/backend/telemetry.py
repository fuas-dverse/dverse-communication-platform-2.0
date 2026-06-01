import os

from opentelemetry import metrics, trace
from opentelemetry.sdk.metrics import MeterProvider
from opentelemetry.sdk.metrics.export import PeriodicExportingMetricReader
from opentelemetry.sdk.resources import SERVICE_NAME, Resource
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import BatchSpanProcessor


def setup_telemetry() -> bool:
    """Configure OTel providers and auto-instrumentors. Returns True when enabled."""
    enabled = os.environ.get("OTEL_ENABLED", "false").lower() in ("1", "true", "yes")
    if not enabled:
        return False

    endpoint = os.environ.get("OTEL_EXPORTER_OTLP_ENDPOINT", "http://localhost:4318")
    service_name = os.environ.get("OTEL_SERVICE_NAME", "chatapp-backend")
    resource = Resource.create({SERVICE_NAME: service_name})

    from opentelemetry.exporter.otlp.proto.http.metric_exporter import OTLPMetricExporter
    from opentelemetry.exporter.otlp.proto.http.trace_exporter import OTLPSpanExporter

    tracer_provider = TracerProvider(resource=resource)
    tracer_provider.add_span_processor(
        BatchSpanProcessor(OTLPSpanExporter(endpoint=f"{endpoint}/v1/traces"))
    )
    trace.set_tracer_provider(tracer_provider)

    reader = PeriodicExportingMetricReader(
        OTLPMetricExporter(endpoint=f"{endpoint}/v1/metrics")
    )
    metrics.set_meter_provider(MeterProvider(resource=resource, metric_readers=[reader]))

    from opentelemetry.instrumentation.httpx import HTTPXClientInstrumentor
    from opentelemetry.instrumentation.sqlite3 import SQLite3Instrumentor

    SQLite3Instrumentor().instrument()
    HTTPXClientInstrumentor().instrument()

    print(f"[OTel] Telemetry enabled — exporting to {endpoint} as '{service_name}'")
    return True


# Proxy tracer/meter: no-op when setup_telemetry() is not called,
# real when it is (OTel's ProxyTracerProvider/ProxyMeterProvider handles the switch).
tracer = trace.get_tracer("chatapp")
meter = metrics.get_meter("chatapp")

llm_duration = meter.create_histogram(
    "chatapp.llm.duration",
    unit="s",
    description="LLM API call duration",
)
llm_requests = meter.create_counter(
    "chatapp.llm.requests",
    description="Total LLM API requests",
)
zenoh_duration = meter.create_histogram(
    "chatapp.zenoh.duration",
    unit="s",
    description="Zenoh bot request round-trip duration",
)
