# DVerse Communication Platform 2.0

> A distributed multi-agent social communication platform for humans and AI, built for small groups to collaborate, co-create, and decide together.

Part of the Interaction Design (IXD) Research Group at Fontys ICT, Eindhoven. Supervised by Marc van Grootel.

---

## Table of Contents

- [DVerse Communication Platform 2.0](#dverse-communication-platform-20)
  - [Table of Contents](#table-of-contents)
  - [Overview](#overview)
  - [Repository Layout](#repository-layout)
  - [Tech Stack](#tech-stack)
  - [Current State](#current-state)
  - [Running the App](#running-the-app)
    - [0. (Quickest) Start everything with Docker](#0-quickest-start-everything-with-docker)
    - [1. Start infrastructure](#1-start-infrastructure)
    - [2. Start the backend](#2-start-the-backend)
    - [3. Start the frontend](#3-start-the-frontend)
    - [4. (Optional) Start telemetry](#4-optional-start-telemetry)
    - [5. (Optional) Run bot agents for A2A](#5-optional-run-bot-agents-for-a2a)
    - [6. (Optional) Run the Tauri desktop launcher](#6-optional-run-the-tauri-desktop-launcher)
  - [Architecture Decision Records](#architecture-decision-records)
  - [CI/CD \& Code Quality](#cicd--code-quality)
  - [Collaboration with other DVerse Groups](#collaboration-with-other-dverse-groups)
  - [Contributors](#contributors)

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

### 0. (Quickest) Start everything with Docker

```bash
cd experiments/chat-app
docker compose up
```

This starts backend, frontend, Zenoh router, Ollama, and all telemetry services in one command.
- UI: http://localhost:5173
- API: http://localhost:8080
- Grafana: http://localhost:3000
- Jaeger: http://localhost:16686

### 1. Pull the LLM model (first run only)

Ollama starts automatically with `docker compose up`. After first boot, pull the model:

```bash
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

### 6. (Optional) Run the Tauri desktop launcher

The `src/zenoh_client_launcher` app manages Zenoh sessions with mTLS. Requires Rust and Node installed.

If you don't have the Tauri CLI yet:

```bash
cargo install tauri-cli
```

Then run:

```bash
cd src/zenoh_client_launcher
cargo tauri dev
```

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

| Name | Role                                                                                                                   |
|---|---------------------------------------------------------------------------------------------------------------------------|
| Abel-Raul Mazilu | A2A Communication, Observability, CI/CD, UI/UX, Zenoh, backend development (Rust and Python)               |
| Yordan Mitev     | Tauri client, Rust, Zenoh, mTLS security                                                                   |
| Denis Neagoe     | Tauri client, Proof-of-concept, AI-Human and AI-AI communication, Zenoh Session bridge. Dockerization      |

**Supervisor:** Marc van Grootel — Fontys ICT, Interaction Design Research Group

---

*DVerse Communication Platform 2.0 · Fontys ICT · Interaction Design Research Group · 2026*
