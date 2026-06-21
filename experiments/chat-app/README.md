# ChatApp

A full-stack chat workspace with user authentication, servers, rooms, real-time messaging, AI bots, and an Agent-to-Agent (A2A) council mode where two AI agents debate over a Zenoh message bus.

## Architecture

```
┌─────────────┐        HTTP / SSE         ┌──────────────────────┐
│  React UI   │ ◄────────────────────────► │  FastAPI Backend     │
│ (Vite)      │      localhost:8080        │  (Python 3.11+)      │
└─────────────┘                            │  SQLite DB           │
                                           └──────────┬───────────┘
                                                      │ Zenoh pub/sub
                                           ┌──────────▼───────────┐
                                           │  Zenoh Router        │
                                           │  (Docker / local)    │
                                           └──────────┬───────────┘
                                                      │
                              ┌───────────────────────┼───────────────────────┐
                              │                       │                       │
                   ┌──────────▼──────┐   ┌───────────▼──────┐   ┌────────────▼──────┐
                   │  bot_agent.py   │   │  bot_agent.py    │   │  any external     │
                   │  (bot-1)        │   │  (bot-2)         │   │  Zenoh agent      │
                   └─────────────────┘   └──────────────────┘   └───────────────────┘
```

**Message flow for bot replies:**
1. User sends `@botname message` → backend saves message, publishes SSE event
2. Backend publishes `chat/{room_id}/request/{botname}` on Zenoh
3. Bot agent receives request, calls its LLM, publishes `chat/response/{request_id}`
4. Backend resolves the pending future, updates the DB, broadcasts updated message via SSE

**A2A Council flow:**
1. User sends `/zenoh @bot1 @bot2 [turns] topic`
2. Backend alternates between bot1 and bot2 for `turns` rounds over Zenoh
3. Each turn is saved and streamed to all clients in real time
4. After the final turn, bot1 generates a synthesis summary

## Features

- User registration and login with JWT auth
- Servers with membership, online presence, and invite codes
- Rooms inside servers or as standalone spaces
- Real-time updates over Server-Sent Events (SSE)
- Bot-triggered replies by mentioning a bot with `@name`
- Bot providers: `claude` (Anthropic API), `local` (Ollama), `zenoh` (external agent)
- Bot personalities: `assistant`, `coder`, `creative`, `analyst`
- **A2A Council** — `/zenoh @bot1 @bot2 [turns] topic` starts a structured multi-turn debate between two bots over Zenoh
- Zenoh bot presence — external agents advertise themselves via heartbeat; `GET /bots/available` lists online bots
- Distributed tracing via OpenTelemetry (Jaeger) and structured logging via Logfire
- Grafana dashboard for HTTP, LLM, Zenoh, and A2A metrics

## Project Layout

```
experiments/chat-app/
├── backend/                # FastAPI API, SQLite, bot orchestration
│   ├── models/             # Pydantic models (bot, message, room, user)
│   ├── routes/             # HTTP route handlers (auth, rooms, messages, servers)
│   ├── services/
│   │   ├── a2a.py          # A2A Council session orchestrator
│   │   ├── llm.py          # LLM call helpers (Claude, Ollama)
│   │   ├── sse.py          # Server-Sent Events broker
│   │   └── zenoh_bridge.py # Zenoh pub/sub bridge (request/response pattern)
│   ├── tests/              # pytest test suite
│   ├── db.py               # SQLite schema + migrations
│   ├── main.py             # FastAPI app entry point
│   └── requirements.txt
├── frontend/               # React + Vite UI
│   ├── src/
│   │   ├── api/            # API client (fetch wrappers)
│   │   ├── components/     # MessageInput, MessageList, BotSettings, etc.
│   │   ├── pages/          # ChatRoomPage, LoginPage, RoomsPage, etc.
│   │   └── store/          # Auth state
│   └── tests/              # Vitest frontend tests
├── bot_agent.py            # Standalone Zenoh bot agent (run one per bot)
├── docker-compose.yml      # Zenoh router, Ollama, Jaeger, OTel, Prometheus, Grafana
├── .env.example            # Environment variable template
└── scripts/
    ├── build.sh            # Build bot_agent binary (Linux/macOS)
    └── build.bat           # Build bot_agent binary (Windows)
```

## Requirements

- Python 3.11+
- Node.js 18+ (or Bun)
- Docker (for Zenoh router, Ollama, and telemetry stack)
- `uv` (optional, for running `bot_agent.py` without a Python install)

## Configuration

Copy `.env.example` to `.env` in `experiments/chat-app/` and fill in the values.

### Core variables

| Variable | Default | Description |
|---|---|---|
| `SECRET_KEY` | — | JWT signing secret. **Required.** Use a long random string. |
| `ANTHROPIC_API_KEY` | — | Required for `provider=claude` bots. |
| `ZENOH_ROUTER` | `tcp/localhost:7447` | Zenoh router endpoint. |
| `LOCAL_LLM_URL` | `http://localhost:11434` | Ollama or OpenAI-compatible endpoint for `provider=local` bots. |
| `LOCAL_LLM_MODEL` | `deepseek-r1:1.5b` | Model name for local inference. |
| `LOCAL_LLM_TIMEOUT_SECONDS` | `240` | Timeout for local LLM calls (seconds). |
| `BOT_HISTORY_LIMIT` | `40` | Recent messages included in bot context. |
| `BOT_DEBUG_CONTEXT` | `false` | Log bot prompt payload to backend console. |
| `MAX_BOT_HOPS` | `4` | Max bot-to-bot chaining depth for `@mention` chains. |

### Logfire variables (optional)

| Variable | Default | Description |
|---|---|---|
| `LOGFIRE_TOKEN` | — | Logfire project token. Leave unset to disable. |
| `LOGFIRE_SERVICE_NAME` | `chatapp-backend` | Service name shown in Logfire. |
| `LOGFIRE_ENVIRONMENT` | `development` | Environment tag. |
| `LOGFIRE_PYDANTIC_RECORD` | `all` | Pydantic model recording mode. |

### OpenTelemetry variables (optional)

| Variable | Default | Description |
|---|---|---|
| `OTEL_ENABLED` | `false` | Set to `true` to activate OTLP tracing and metrics export. |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://localhost:4318` | OTLP collector endpoint. |
| `OTEL_SERVICE_NAME` | `chatapp-backend` | Service name in traces. |

## Running Locally

### 1. Start infrastructure services

```bash
cd experiments/chat-app

# Zenoh router + Ollama (minimum for bot support)
docker compose up zenoh-router ollama -d

# Pull the default model into Ollama
docker exec chatapp-ollama ollama pull deepseek-r1:1.5b
```

### 2. (Optional) Start telemetry stack

```bash
docker compose up jaeger otelcol prometheus grafana -d
```

Set `OTEL_ENABLED=true` in `.env` to start exporting traces and metrics.

- Grafana: http://localhost:3000 (anonymous access, Viewer role)
- Jaeger: http://localhost:16686
- Prometheus: http://localhost:9090

### 3. Start the backend

```bash
cd experiments/chat-app
pip install -r backend/requirements.txt
python run.py
```

Backend runs on `http://localhost:8080`. The SQLite database (`chatapp.db`) is created automatically on first run.

Alternatively with uvicorn directly:

```bash
python -m uvicorn backend.main:app --reload --port 8080
```

### 4. Start the frontend

```bash
cd experiments/chat-app/frontend
npm install
npm run dev
```

UI runs on `http://localhost:5173`.

### 5. Run bot agents (for Zenoh / A2A features)

Each bot you want to use via Zenoh needs its own agent process. The bot name must match the `@name` you configure in the room UI.

```bash
cd experiments/chat-app

# Terminal 1
uv run bot_agent.py --name bot-1 --description "First agent"

# Terminal 2
uv run bot_agent.py --name bot-2 --description "Second agent"
```

On first run without `--token`, a random token is printed. Users need this token to add the bot to a room via the UI.

## Running Tests

### Backend (pytest)

```bash
cd experiments/chat-app
pip install -r backend/requirements.txt
python -m pytest backend/tests/ -v
```

### Frontend (Vitest)

```bash
cd experiments/chat-app/frontend
npm install
npm run test
```

## Using the A2A Council

The A2A Council lets two Zenoh-connected bots hold a structured debate on a topic, with all turns streamed live to the room.

**Syntax:**
```
/zenoh @bot1 @bot2 [turns] topic
```

- `bot1`, `bot2` — bot names as configured in the room (case-insensitive)
- `turns` — optional number of back-and-forth rounds (1–10, default 3)
- `topic` — the prompt or question to debate

**Example:**
```
/zenoh @bot-1 @bot-2 4 Should AI be regulated?
```

**What happens:**
1. Backend starts a session: bot1 responds to the topic, bot2 responds to bot1, and so on for `turns` rounds
2. Each response appears in the room in real time
3. After the last round, bot1 generates a synthesis summary
4. All communication between the backend and the agents travels over the Zenoh message bus

**Requirements:** both bots must be configured in the room with `provider=zenoh`, and their agent processes must be running and connected to the Zenoh router.

## Bot Setup Guide

1. Start the Zenoh router: `docker compose up zenoh-router -d`
2. Run a bot agent: `uv run bot_agent.py --name mybot` — copy the printed token
3. In the UI, open a room → Bot Settings → Add Bot
   - Set name to `mybot` (must match `--name` exactly)
   - Set provider to `zenoh`
   - Paste the token
4. Mention the bot in chat with `@mybot hello`
5. For A2A, add a second bot the same way, then use `/zenoh @mybot @otherbot topic`

## Zenoh Bot Agent Options

| Flag | Env var | Default | Description |
|---|---|---|---|
| `--name` | `BOT_NAME` | `mybot` | Bot handle used in @mentions |
| `--router` | `ZENOH_ROUTER` | `tcp/localhost:7447` | Zenoh router to connect to |
| `--model` | `OLLAMA_MODEL` | `deepseek-r1:1.5b` | Ollama model name |
| `--ollama-url` | `OLLAMA_URL` | `http://localhost:11434` | Ollama base URL |
| `--description` | `BOT_DESCRIPTION` | — | Short description shown in bot picker |
| `--platform` | `BOT_PLATFORM` | `zenoh` | Platform identifier |
| `--token` | `BOT_TOKEN` | random | Secret token users supply when adding the bot |

To use Claude instead of Ollama, set `ANTHROPIC_API_KEY` as an environment variable and edit the `call_llm()` function in `bot_agent.py` to call the Anthropic API.

## API Reference

| Method | Path | Description |
|---|---|---|
| `POST` | `/auth/register` | Register a new user |
| `POST` | `/auth/login` | Login, returns JWT |
| `GET` | `/servers` | List joined servers |
| `POST` | `/servers` | Create a server |
| `GET` | `/rooms` | List rooms |
| `POST` | `/rooms` | Create a room |
| `GET` | `/rooms/{id}/messages` | Get message history |
| `POST` | `/rooms/{id}/messages` | Send a message (triggers bot if `@mention` or `/zenoh` command) |
| `GET` | `/rooms/{id}/stream` | SSE stream for live message updates |
| `GET` | `/bots/available` | List Zenoh bots currently online (heartbeat-based) |

Interactive API docs available at `http://localhost:8080/docs` when the backend is running.

## Observability

The Grafana dashboard (`http://localhost:3000`) shows four sections:

- **HTTP API** — request rate and p50/p95 latency per endpoint
- **LLM Calls** — request rate, response duration, token consumption, and error rate per provider
- **Zenoh Bot** — request rate and round-trip latency per bot name
- **A2A Council** — session rate, turns completed per session, and session wall-clock duration

The dashboard is auto-provisioned from `grafana/provisioning/` — no manual setup needed.

## Known Limitations

- SQLite is not suitable for multi-instance deployments. For production use, migrate to Postgres.
- CORS is hardcoded to `http://localhost:5173`. Update `main.py` for other origins.
- The Zenoh router runs without TLS in the default docker-compose setup. For production use, configure TLS (see the `zenoh_client_launcher` app for an example with mTLS).
- Bot agent processes must be restarted manually if they crash. No supervisor/watchdog is included.
