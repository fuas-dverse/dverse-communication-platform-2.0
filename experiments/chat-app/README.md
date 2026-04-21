# ChatApp

ChatApp is a full-stack chat workspace with user authentication, servers, rooms, live message updates, and AI bots. The backend is a FastAPI service backed by SQLite, and the frontend is a Vite + React app.

## Features

- User registration and login with JWT auth
- Servers with membership, online presence, and invite codes
- Rooms inside servers or as standalone spaces
- Real-time updates over Server-Sent Events
- Bot-triggered replies by mentioning a bot with `@name`
- Bot providers for Claude, local LLMs, and Zenoh-connected agents

## Project Layout

- `backend/` - FastAPI API, SQLite data store, and bot orchestration
- `frontend/` - React UI for chat, rooms, servers, and bot settings
- `bot_agent.py` - Optional Zenoh bot agent example
- `docker-compose.yml` - Local Zenoh router and Ollama services

## Requirements

- Python 3.11+
- Node.js 18+
- SQLite is created automatically on first run
- Optional: Docker for Zenoh and Ollama

## Configuration

The backend reads environment variables from a `.env` file in `experiments/chat-app/`.

Common variables:

- `SECRET_KEY` - JWT signing secret
- `ANTHROPIC_API_KEY` - Required for Claude-backed bots
- `LOCAL_LLM_URL` - Local model endpoint, defaults to `http://localhost:11434`
- `LOCAL_LLM_MODEL` - Default local model name, defaults to `deepseek-r1:1.5b`
- `LOCAL_LLM_TIMEOUT_SECONDS` - Local model request timeout
- `ZENOH_ROUTER` - Zenoh router endpoint, defaults to `tcp/localhost:7447`
- `BOT_HISTORY_LIMIT` - Number of recent messages included in bot context
- `BOT_DEBUG_CONTEXT` - Set to `true` to print bot prompt context

The frontend targets `http://localhost:8000` by default in `frontend/src/api/client.ts`.

## Run Locally

### 1. Start supporting services

If you want Zenoh or local LLM support, start the bundled services:

```bash
docker compose up -d
```

This starts:

- Zenoh router on `tcp/localhost:7447`
- Ollama on `http://localhost:11434`

### 2. Start the backend

From `experiments/chat-app/`:

```bash
cd backend
python -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
python run.py
```

The API runs on `http://localhost:8000`.

### 3. Start the frontend

From `experiments/chat-app/frontend/`:

```bash
npm install
npm run dev
```

The UI runs on `http://localhost:5173`.

## Optional: Zenoh Bot Agent

The repo includes an example Zenoh-connected bot agent in `bot_agent.py`. Run it with a matching bot name to enable `@botname` replies in rooms.

```bash
cd experiments/chat-app
python bot_agent.py --name mybot
```

You can also run it with `uv`:

```bash
uv run bot_agent.py --name mybot
```

Set `BOT_NAME`, `ZENOH_ROUTER`, and `ANTHROPIC_API_KEY` if you prefer environment variables.

## Notes

- The backend enables CORS for `http://localhost:5173`.
- The SQLite database file is created automatically at `chatapp.db` in the project root.
- Bot providers are defined in `backend/models/bot.py`: `claude`, `local`, and `zenoh`.
