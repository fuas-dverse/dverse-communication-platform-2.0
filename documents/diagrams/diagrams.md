# DVerse Platform — Technical Diagrams

---

## 1. CI/CD Pipeline

```mermaid
flowchart TD
    subgraph triggers["Triggers"]
        P1[Push: develop / feature/*]
        P2[PR to main]
        P3[Tag: sprint-*]
        P4[Tag: v*.*.*]
    end

    subgraph chat_ci["chat-app-build.yml  (main app)"]
        direction TB
        C1[Checkout + Setup Python 3.12\nSetup Bun]

        subgraph backend_job["Job: backend"]
            B1[pip install requirements]
            B2[ruff check — lint]
            B3[python -m py_compile — syntax]
            B4[unittest + coverage]
            B5[Upload coverage → Codacy]
            B1 --> B2 --> B3 --> B4 --> B5
        end

        subgraph frontend_job["Job: frontend"]
            F1[bun install]
            F2[vitest run — unit tests]
            F3[bun build — bundle validation]
            F1 --> F2 --> F3
        end

        C1 --> backend_job
        C1 --> frontend_job
    end

    subgraph bot_ci["build-bot-agent.yml  (bot binary)"]
        direction TB
        A1[Checkout + Setup Python 3.12]
        A2[pip install + PyInstaller]
        A3{Matrix}
        A4[Build Linux binary]
        A5[Build macOS binary]
        A6[Build Windows binary]
        A7[GitHub Release\nupload artifacts]
        A1 --> A2 --> A3
        A3 -->|ubuntu-latest| A4 --> A7
        A3 -->|macos-latest| A5 --> A7
        A3 -->|windows-latest| A6 --> A7
    end

    subgraph adr_ci["adr-build.yml  (docs)"]
        direction TB
        D1[Checkout]
        D2[LaTeX compile ADR PDFs]
        D3[GitHub Release\npublish PDFs]
        D1 --> D2 --> D3
    end

    P1 & P2 --> chat_ci
    P4       --> bot_ci
    P3       --> adr_ci
```

---

## 2. Observability Stack Architecture

```mermaid
graph TB
    subgraph app["Backend Application  (FastAPI)"]
        MW[main.py startup]
        LF[observability.py\nLogfire SDK]
        OT[telemetry.py\nOpenTelemetry SDK]
        MW --> LF
        MW --> OT
    end

    subgraph instruments["Auto-Instruments"]
        I1[FastAPI requests]
        I2[HTTPX outbound calls]
        I3[SQLite3 queries]
        I4[Pydantic validation]
        I5[Anthropic API calls]
    end

    LF -->|instruments| I1
    LF -->|instruments| I2
    LF -->|instruments| I4
    LF -->|instruments| I5
    OT -->|instruments| I1
    OT -->|instruments| I2
    OT -->|instruments| I3

    subgraph custom_metrics["Custom OTel Metrics"]
        M1[chatapp.llm.duration histogram]
        M2[chatapp.llm.requests counter]
        M3[chatapp.zenoh.duration histogram]
    end

    OT --> custom_metrics

    subgraph logfire_dest["Logfire Destination"]
        LC{LOGFIRE_TOKEN set?}
        LCloud[Logfire Cloud\npydantic.dev/logfire]
        LConsole[Console stdout]
        LC -->|yes| LCloud
        LC -->|no| LConsole
    end

    LF --> LC

    subgraph otel_dest["OTel Export Pipeline"]
        OTEL_CHECK{OTEL_ENABLED\n= true?}
        COLLECTOR[OTel Collector\n:4317 gRPC / :4318 HTTP]
        OTEL_CHECK -->|yes| COLLECTOR
    end

    OT --> OTEL_CHECK

    subgraph collector_pipelines["Collector Pipelines"]
        BATCH[Batch Processor]
        JAEGER_EXP[Jaeger OTLP Exporter\n→ chatapp-jaeger:4317]
        PROM_EXP[Prometheus Exporter\n→ 0.0.0.0:8889]
    end

    COLLECTOR --> BATCH
    BATCH -->|traces pipeline| JAEGER_EXP
    BATCH -->|metrics pipeline| PROM_EXP

    subgraph backends["Observability Backends"]
        JAEGER[Jaeger\n:16686 UI]
        PROMETHEUS[Prometheus\n:9090 — scrapes :8889]
        GRAFANA[Grafana\n:3000 dashboards]
    end

    JAEGER_EXP --> JAEGER
    PROM_EXP -->|scraped by| PROMETHEUS
    PROMETHEUS -->|datasource| GRAFANA
```

---

## 3. Docker Compose Service Dependency Graph

```mermaid
graph LR
    subgraph network["chatapp bridge network"]
        ZENOH[chatapp-zenoh\nZenoh Router\n:8000 REST\n:7447 tcp/udp]
        OLLAMA[chatapp-ollama\nLocal LLM\n:11434]
        JAEGER[chatapp-jaeger\nJaeger all-in-one\n:16686 UI\n:4317 OTLP]
        OTELCOL[chatapp-otelcol\nOTel Collector\n:4317 gRPC\n:4318 HTTP\n:8889 metrics]
        PROMETHEUS[chatapp-prometheus\n:9090]
        GRAFANA[chatapp-grafana\n:3000]
    end

    OTELCOL -->|depends_on| JAEGER
    PROMETHEUS -->|depends_on| OTELCOL
    GRAFANA -->|depends_on| PROMETHEUS

    OTELCOL -->|exports traces| JAEGER
    OTELCOL -->|exposes metrics| PROMETHEUS
    PROMETHEUS -->|datasource query| GRAFANA

    APP[Backend App\n:8000] -.->|OTLP HTTP :4318\nif OTEL_ENABLED| OTELCOL
    APP -.->|Logfire cloud\nif token set| LOGFIRE_CLOUD[Logfire Cloud]
    APP -.->|Zenoh pub/sub\n:7447| ZENOH
    APP -.->|LLM calls\n:11434| OLLAMA

    style APP fill:#2d6a9f,color:#fff
    style LOGFIRE_CLOUD fill:#7c3aed,color:#fff
```

---

## 4. Telemetry Data Flow (Sequence)

```mermaid
sequenceDiagram
    participant Client as HTTP Client
    participant FastAPI as FastAPI App
    participant LF as Logfire SDK
    participant OT as OTel SDK
    participant COL as OTel Collector
    participant JAG as Jaeger
    participant PROM as Prometheus
    participant GF as Grafana
    participant LFC as Logfire Cloud

    Client->>FastAPI: HTTP Request

    activate FastAPI
    note over FastAPI,LF: Logfire auto-instrument fires
    FastAPI->>LF: span start (request)
    FastAPI->>LF: log request metadata

    alt OTEL_ENABLED=true
        note over FastAPI,OT: OTel auto-instrument fires
        FastAPI->>OT: span start (request)
    end

    FastAPI->>FastAPI: route handler
    FastAPI->>LF: log DB queries / LLM calls
    FastAPI->>OT: record chatapp.llm.duration
    FastAPI->>OT: increment chatapp.llm.requests

    FastAPI->>LF: span end
    FastAPI-->>Client: HTTP Response
    deactivate FastAPI

    par Logfire export
        LF->>LFC: traces + logs (OTLP)
    and OTel export (if enabled)
        OT->>COL: spans via OTLP HTTP :4318
        OT->>COL: metrics via OTLP HTTP :4318
        COL->>JAG: traces (OTLP gRPC)
        COL-->>PROM: metrics exposed on :8889
    end

    PROM->>COL: scrape /metrics every 15s
    GF->>PROM: PromQL queries
    GF-->>Client: dashboard panels
```

---

## 5. Grafana Dashboard Panels (Metric Sources)

```mermaid
graph TD
    subgraph grafana["Grafana — chatapp dashboard"]
        subgraph http_panel["HTTP API"]
            P1[Request Rate\nrate(http_server_request_duration_seconds_count[5m])]
            P2[Latency p50\nhistogram_quantile(0.50 ...)]
            P3[Latency p95\nhistogram_quantile(0.95 ...)]
        end

        subgraph llm_panel["LLM Calls"]
            P4[Requests by Provider\nrate(chatapp_llm_requests_total[5m])\nby provider]
            P5[Requests by Status\nrate(chatapp_llm_requests_total[5m])\nby status]
        end

        subgraph zenoh_panel["Zenoh Bot Requests"]
            P6[Bot Request Duration\nhistogram_quantile(0.95,\nrate(chatapp_zenoh_duration_seconds_bucket[5m]))]
        end
    end

    PROM[Prometheus :9090] -->|default datasource| grafana
```

---

## 6. Logfire Observability Flow

```mermaid
flowchart LR
    subgraph sources["Instrumented Sources"]
        S1[FastAPI\nrequest spans]
        S2[HTTPX\noutbound HTTP]
        S3[Pydantic\nvalidation events]
        S4[Anthropic SDK\nLLM call spans]
        S5[Manual logfire.span()\nlogfire.info() calls]
    end

    subgraph scrubbing["Scrubbing Rules"]
        SC[Redact fields:\npassword / secret / token\napi_key / password_hash\naccess_token]
    end

    subgraph config["Config (observability.py)"]
        ENV{LOGFIRE_TOKEN\npresent?}
        CLOUD[Logfire Cloud\nService: chatapp-backend\nEnv: development\nPydantic recording: all]
        CONSOLE[Console exporter\nlocal dev fallback]
    end

    S1 & S2 & S3 & S4 & S5 --> scrubbing
    scrubbing --> ENV
    ENV -->|yes| CLOUD
    ENV -->|no| CONSOLE

    CLOUD --> VIZ[Logfire UI\nTrace explorer\nStructured logs\nPydantic validation view]
```

---

## 7. Bot Agent Build & Distribution Pipeline

```mermaid
flowchart TD
    DEV[Developer pushes\ntag v*.*.*]
    
    subgraph matrix["Matrix build  (3 parallel jobs)"]
        L[ubuntu-latest\nPyInstaller → Linux binary]
        M[macos-latest\nPyInstaller → macOS binary]
        W[windows-latest\nPyInstaller → Windows .exe]
    end

    DEV --> matrix

    REL[GitHub Release\nattach all 3 artifacts]
    L & M & W --> REL

    subgraph users["End users"]
        UL[Linux user\ndownloads binary]
        UM[macOS user\ndownloads binary]
        UW[Windows user\ndownloads .exe]
    end

    REL --> UL & UM & UW
```
