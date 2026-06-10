# ChatApp

ChatApp is a full-stack chat workspace with user authentication, servers, rooms, live message updates, and AI bots. The backend is a FastAPI service backed by SQLite, and the frontend is a Vite + React app.

## Features

- User registration and login with JWT auth

- Servers with membership, online presence, and invite codes

- Rooms inside servers or as standalone spaces

- Real-time updates over Server-Sent Events

- Bot-triggered replies by mentioning a bot with @name

- Bot providers: Claude, local LLMs (Ollama), and Zenoh-connected external agents

- Bot personalities: assistant, coder, creative, analyst

- Zenoh bot presence — external agents advertise themselves via heartbeat; `GET /bots/available` lists online bots

- Distributed tracing via OpenTelemetry (Jaeger) and structured logging via Logfire

## Project Layout

- backend/ — FastAPI API, SQLite data store, and bot orchestration
- frontend/ — React UI for chat, rooms, servers, and bot settings
- bot_agent.py — Standalone Zenoh bot agent (run on any machine)
- docker-compose.yml — Local Zenoh router, Ollama, Jaeger, OpenTelemetry Collector, Prometheus, Grafana

## Requirements

- Python 3.11+
- Node.js 18+
- SQLite is created automatically on first run
- Optional: Docker for Zenoh, Ollama, and telemetry stack

## Configuration

The backend reads environment variables from a `.env` file in `experiments/chat-app/`. Copy `.env.example` as a starting point.

### Core variables

| Variable | Default | Description |
|---|---|---|
| `SECRET_KEY` | — | JWT signing secret. Use a long random string. |
| `ANTHROPIC_API_KEY` | — | Required for `provider=claude` bots. |
| `ZENOH_ROUTER` | `tcp/localhost:7447` | Zenoh router endpoint for external bot agents. |
| `LOCAL_LLM_URL` | `http://localhost:11434` | Ollama or OpenAI-compatible endpoint for `provider=local` bots. |
| `LOCAL_LLM_MODEL` | `deepseek-r1:1.5b` | Model name for local inference. |
| `LOCAL_LLM_TIMEOUT_SECONDS` | `240` | Request timeout for local LLM calls. |
| `BOT_HISTORY_LIMIT` | `40` | Recent messages included in bot context. |
| `BOT_DEBUG_CONTEXT` | `false` | Print bot prompt payload in backend logs. |

### Logfire variables (optional)

Logfire provides structured logging, Pydantic model recording, HTTPX tracing, and Anthropic SDK tracing. When `LOGFIRE_TOKEN` is absent the backend runs without sending data to Logfire.

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

The frontend targets `http://localhost:8080` by default in `frontend/src/api/client.ts`.

## Run Locally

### 1. Start supporting services

If you want Zenoh or local LLM support, start the bundled services:

```bash
docker compose up -d
```

This starts:

- Zenoh router on `tcp/localhost:7447`
- Ollama on `http://localhost:11434`

### 2. Start telemetry services (optional)

To enable distributed tracing and metrics (Jaeger, OpenTelemetry Collector, Prometheus, Grafana):

```bash
docker compose up jaeger otelcol prometheus grafana -d
```

Set `OTEL_ENABLED=true` in your `.env` file to activate telemetry in the backend.

### 3. Start the backend

From `experiments/chat-app/`:

```bash
pip install -r backend/requirements.txt
python run.py
```

The API runs on `http://localhost:8080`.

Alternatively with uvicorn directly:

```bash
python -m uvicorn backend.main:app --reload --port 8080
```

### 4. Start the frontend

From `experiments/chat-app/frontend/`:

```bash
npm install
npm run dev
```

The UI runs on `http://localhost:5173`.

## Zenoh Bot Agent

The repo includes a standalone Zenoh-connected bot agent in `bot_agent.py`. It connects to the Zenoh router, advertises itself via a heartbeat on `chat/presence/<name>`, and responds to messages on `chat/*/request/<name>`.

Run with uv (no Python install needed):

```bash
cd experiments/chat-app
uv run bot_agent.py --name mybot --description "My assistant bot"
```

Or with plain Python:

```bash
python bot_agent.py --name mybot
```

On first run without `--token` / `BOT_TOKEN` set, a random token is generated and printed. Share this token with room users — they need it to add the bot to a room.

Full options:

| Flag | Env var | Default | Description |
|---|---|---|---|
| `--name` | `BOT_NAME` | `mybot` | Bot handle used in @mentions |
| `--router` | `ZENOH_ROUTER` | `tcp/localhost:7447` | Zenoh router to connect to |
| `--model` | `OLLAMA_MODEL` | `deepseek-r1:1.5b` | Ollama model name |
| `--ollama-url` | `OLLAMA_URL` | `http://localhost:11434` | Ollama base URL |
| `--description` | `BOT_DESCRIPTION` | — | Short description shown in bot picker |
| `--platform` | `BOT_PLATFORM` | `zenoh` | Platform identifier |
| `--token` | `BOT_TOKEN` | random | Secret token users supply when adding the bot |

## API Endpoints

| Method | Path | Description |
|---|---|---|
| `POST` | `/auth/register` | Register a new user |
| `POST` | `/auth/login` | Login, returns JWT |
| `GET` | `/servers` | List joined servers |
| `POST` | `/servers` | Create a server |
| `GET` | `/rooms` | List rooms |
| `POST` | `/rooms` | Create a room |
| `GET` | `/rooms/{id}/messages` | Get message history |
| `POST` | `/rooms/{id}/messages` | Send a message (triggers bot if @mention) |
| `GET` | `/rooms/{id}/events` | SSE stream for live message updates |
| `GET` | `/bots/available` | List Zenoh bots currently online (heartbeat-based) |

## Notes

- The backend enables CORS for `http://localhost:5173`.
- SQLite database is created automatically at `chatapp.db` in the project root.
- Bot providers: `claude`, `local`, `zenoh` — set per bot in the UI or API.
- Bot personalities: `assistant`, `coder`, `creative`, `analyst` — affects the system prompt sent to the LLM.
- Zenoh bot presence uses a 90-second TTL; bots must heartbeat every 30 seconds to stay listed.
