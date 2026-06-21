# DVerse Communication Platform 2.0

> A distributed multi-agent social communication platform for humans and AI, built for small groups to collaborate, co-create, and decide together.

Part of the Interaction Design (IXD) Research Group at Fontys ICT, Eindhoven. Supervised by Marc van Grootel.

---

## Table of Contents

1. [Overview](#overview)
2. [Repository Layout](#repository-layout)
3. [Tech Stack](#tech-stack)
4. [Current State](#current-state)
5. [Running the App](#running-the-app)
6. [Architecture Decision Records](#architecture-decision-records)
7. [CI/CD & Code Quality](#cicd--code-quality)
8. [Collaboration with other DVerse Groups](#collaboration-with-other-dverse-groups)
9. [Contributors](#contributors)

---

## Overview

DVerse Communications Platform is a social network in which humans and AI cooperate. The platform supports small groups in real-time communication, AI-assisted interaction, and structured decision making.

This iteration takes the **adventurous route**: a ground-up rearchitecture using Rust and Zenoh, replacing the prior Python/NATS stack with a high-performance pub-sub communication mesh. AI agents are communicating with users through the chat interface and with each other directly over Zenoh topics via the **A2A (Agent-to-Agent) Council** protocol.

---

## Repository Layout

```
dverse-communication-platform-2.0/
├── experiments/
│   ├── chat-app/           ← Main runnable experiment (see its own README)
│   ├── zenoh-chat-abel/    ← Early Zenoh chat proof-of-concept
│   ├── zenoh-ping-pong/    ← Zenoh pub-sub latency test
│   ├── mini-ls-application/← Mini CLI experiment
│   └── mini-tree-command/  ← Mini CLI experiment
│
├── src/                    ← Rust workspace crates
│   ├── zenoh_client_launcher/ ← Tauri desktop app (launches bot agents, manages Zenoh sessions)
│   ├── zenoh_router/       ← Embedded Zenoh router with mTLS, ACL, and mDNS discovery
│   ├── bot_framework/      ← Shared bot config types (Rust)
│   ├── a2a/                ← A2A protocol primitives (Rust)
│   ├── agent_a / agent_b   ← Prototype Rust agent implementations
│   ├── ping / pong         ← Zenoh round-trip test harnesses
│   └── ...
│
├── documents/
│   ├── adr/                ← Architecture Decision Records
│   ├── diagrams/           ← System and sequence diagrams
│   ├── research/           ← Research notes and references
│   └── wireframes/         ← UI wireframes
│
├── Cargo.toml              ← Rust workspace root
└── README.md               ← This file
```

---

## Tech Stack

| Layer | Technology | Notes                                           |
|---|---|-------------------------------------------------|
| Chat backend | `Python / FastAPI` | Main experiment backend                         |
| Core Zenoh router | `Rust` | Embedded in Tauri launcher, handles mTLS and ACL |
| Agent framework | `Python` | `bot_agent.py` Zenoh-connected LLM agent        |
| Messaging | `Zenoh` | Pub-sub communication mesh across all components |
| Data validation | `Pydantic` | Schema enforcement across layers                |
| Tracing | `OpenTelemetry` | Distributed traces (Jaeger)                     |
| Structured logging | `Pydantic Logfire` | Schema-validated logs                           |
| Desktop launcher | `Tauri (Rust)` | Manages bot processes and Zenoh sessions        |
| Transport security | `mTLS / PKI` | Mutual TLS on Zenoh connections (Tauri router)  |
| CI/CD | `GitHub Actions` | Automated testing and deployment                |
| Code quality | `Codacy` | Static analysis on every push                   |

---

## Current State

The **main runnable experiment** is `experiments/chat-app/`, a full-stack chat app with:
- JWT auth, servers, rooms, real-time SSE updates
- Local AI bots (Ollama LLM) for AI-assisted communication
- **A2A Council** (`/zenoh @bot1 @bot2 [turns] topic`) — structured bot-to-bot debate over Zenoh, streamed live to the room
- Grafana dashboard for HTTP, LLM, Zenoh, and A2A metrics

The `src/zenoh_client_launcher` Tauri app is a separate desktop client for managing Zenoh sessions with mTLS. It is not required to run the chat-app experiment but is part of the broader platform vision.

---

## Running the App

The full setup guide is in [`experiments/chat-app/README.md`](experiments/chat-app/README.md). Quick start:

### 1. Start infrastructure

```bash
cd experiments/chat-app
docker compose up zenoh-router ollama -d
docker exec chatapp-ollama ollama pull deepseek-r1:1.5b
```

### 2. Start the backend

```bash
cd experiments/chat-app
pip install -r backend/requirements.txt
python run.py
# API: http://localhost:8080
```

### 3. Start the frontend

```bash
cd experiments/chat-app/frontend
npm install
npm run dev
# UI: http://localhost:5173
```

### 4. (Optional) Start telemetry

```bash
cd experiments/chat-app
docker compose up jaeger otelcol prometheus grafana -d
# Grafana: http://localhost:3000
# Jaeger:  http://localhost:16686
```

Set `OTEL_ENABLED=true` in `experiments/chat-app/.env`.

### 5. (Optional) Run bot agents for A2A

```bash
cd experiments/chat-app
uv run bot_agent.py --name bot-1
uv run bot_agent.py --name bot-2   # second terminal
```

Then in the chat: `/zenoh @bot-1 @bot-2 3 Should AI be regulated?`

---

## Architecture Decision Records

All architectural decisions are documented in [`documents/adr/`](documents/adr/).

---

## CI/CD & Code Quality

Every pull request to `main` triggers the CI pipeline via **GitHub Actions**. Code quality is continuously monitored through **Codacy**, enforcing style, complexity, and security checks across both the Rust and Python codebases.

---

## Collaboration with other DVerse Groups

The platform is intended to eventually support the DVerse collaboration game project, providing the underlying communication infrastructure for interactive sessions guided by AI agents.

---

## Contributors

| Name | Role                                                                                         |
|---|----------------------------------------------------------------------------------------------|
| Abel-Raul Mazilu | A2A Communication, Observability, CI/CD, UI/UX, Zenoh, backend development (Rust and Python) |
| Yordan Mitev | Tauri client, Rust, Zenoh, mTLS security                                                     |
| Denis Neagoe | Tauri client, Proof-of-concept, AI-Human and AI-AI communication                             |

**Supervisor:** Marc van Grootel — Fontys ICT, Interaction Design Research Group

---

*DVerse Communication Platform 2.0 · Fontys ICT · Interaction Design Research Group · 2026*
