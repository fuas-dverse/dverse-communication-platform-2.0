# DVerse Communication Platform 2.0

> A distributed multi-agent social communication platform — built for small groups of humans and AI to collaborate, co-create, and decide together.

![CI](https://github.com/fuas-dverse/dverse-communication-platform-2.0/actions/workflows/chat-app-build.yml/badge.svg)
[![Codacy Badge](https://app.codacy.com/project/badge/Grade/175f62f8c904411e97821c10e1e42aa2)](https://app.codacy.com/gh/fuas-dverse/dverse-communication-platform-2.0/dashboard?utm_source=gh&utm_medium=referral&utm_content=&utm_campaign=Badge_grade)
[![Codacy Badge](https://app.codacy.com/project/badge/Coverage/175f62f8c904411e97821c10e1e42aa2)](https://app.codacy.com/gh/fuas-dverse/dverse-communication-platform-2.0/dashboard?utm_source=gh&utm_medium=referral&utm_content=&utm_campaign=Badge_coverage)

Part of the [Interaction Design (IXD) Research Group](https://fuas-dverse.github.io) at Fontys ICT, Eindhoven. Supervised by **Marc van Grootel**.

---

## What is DVerse?

DVerse is a vision of a social network for diverse intelligences — human and artificial. This iteration takes the **adventurous route**: a ground-up rearchitecture using **Rust** and **Zenoh**, replacing the prior Python/NATS stack with a high-performance, peer-to-peer pub-sub mesh.

AI agents participate as first-class citizens — communicating with users through the chat interface and with each other directly over Zenoh topics. A Matrix bridge provides interoperability with existing communication ecosystems.

## Architecture

```
┌──────────────────┐        Zenoh pub/sub        ┌──────────────────────┐
│   Rust Backend   │◄───────────────────────────►│  Python AI Agents    │
│  (core services) │          mTLS / PKI          │  (LLM logic)         │
└────────┬─────────┘                              └──────────────────────┘
         │
         │ HTTP / WebSocket
         ▼
┌──────────────────┐        Matrix bridge         ┌──────────────────────┐
│ React Frontend   │     ┌──────────────┐         │   Matrix Network     │
│  (Vite + React)  │     │  Zenoh Mesh  │◄───────►│  (interoperability)  │
└──────────────────┘     └──────────────┘         └──────────────────────┘
```

All major architectural decisions are documented in [`documents/adr/`](./documents/).

## Tech Stack

| Layer | Technology | Notes |
|---|---|---|
| Core backend | `Rust` | Memory-safe, high-performance services |
| Frontend | `React + Vite` | TypeScript, fast HMR dev experience |
| Agent logic | `Python` | LLM integration and AI-AI communication |
| Messaging | `Zenoh` | Decentralised pub-sub mesh |
| Data validation | `Pydantic` | Schema enforcement across Python layers |
| Tracing | `OpenTelemetry` | Distributed traces across all services |
| Structured logging | `Pydantic Logfire` | Schema-validated logs at system boundaries |
| Protocol bridge | `Matrix ↔ Zenoh` | Ecosystem interoperability |
| Transport security | `mTLS / PKI` | Mutual TLS on all Zenoh connections |
| CI/CD | `GitHub Actions` | Automated build, test, and quality checks |
| Code quality | `Codacy` | Static analysis on every push |

## Getting Started

**Prerequisites:** Rust 1.77+, Python 3.11+, Node.js 18+

```bash
# Clone the repo
git clone https://github.com/fuas-dverse/dverse-communication-platform-2.0.git
cd dverse-communication-platform-2.0

# Build Rust workspace
cargo build

# Install and run the frontend
cd frontend
npm install
npm run dev
```

> For Python agent setup and environment variables, see [`documents/`](./documents/).

## Repository Structure

```
.
├── experiments/            # Isolated Rust & Zenoh experiments
│   ├── chat-app/               # Proof of Concept for the Communications Platform
│   ├── mini-ls-application/    # Rust ls clone (learning project)
│   ├── mini-tree-command/      # Rust tree command
│   ├── zenoh-chat-abel/        # Zenoh pub-sub chat experiment
│   └── zenoh-ping-pong/        # Zenoh ping-pong (pub/sub validation)
├── frontend/               # React + Vite chat UI
├── documents/              # ADRs, wireframes, project documentation
├── .github/workflows/      # CI/CD pipeline definitions
├── Cargo.toml              # Rust workspace manifest
└── package.json            # Frontend dependencies
```

## CI/CD & Code Quality

Every push to `main` and every pull request triggers the GitHub Actions pipeline:

| Stage | What it does |
|---|---|
| `backend-lint` | Runs `ruff` to enforce Python backend code style in `backend/` |
| `backend-test` | Runs unit tests with coverage using `unittest` + `coverage.py` |
| `backend-build` | Validates backend source files (`compileall`, `py_compile`) |
| `frontend-test` | Runs frontend tests using `vitest` with coverage (Bun runtime) |
| `frontend-build` | Builds the frontend using `bun run build` |
| `codacy` | Aggregates backend + frontend coverage and uploads to Codacy (if configured) |

Branch protection requires all stages to pass before merge.

## Experiments

The `experiments/` directory contains standalone Rust and Zenoh projects used to validate technology choices before integrating them into the platform. Each experiment has its own README.

| Experiment | Purpose |
|---|---|
| `chat-app` | DVerse Communications Platform Proof of Concept |
| `mini-ls-application` | Rust fundamentals — I/O, ownership, iterators |
| `mini-tree-command` | Recursive filesystem traversal in Rust |
| `zenoh-chat-abel` | Multi-user pub-sub chat over Zenoh |
| `zenoh-ping-pong` | Latency and reliability validation of Zenoh pub-sub |

## Related Projects

This platform provides the underlying communication infrastructure for the [DVerse Collaboration Game](https://github.com/fuas-dverse/modular-commons) — interactive sessions for groups guided by AI agents.

## Contributors

| Name | Focus |
|---|---|
| [Abel-Raul Mazilu](https://github.com/AbelMazilu) | CI/CD, UI/UX, Zenoh experiments, Rust & Python back-end |
| [Yordan Mitev](https://github.com/YordanMitev) | Rust, Zenoh, mTLS / PKI security |
| [Denis Neagoe](https://github.com/DenisNeagoe) | Zenoh Bridge, AI Context, AI–human & AI–AI communication |

**Supervisor:** Marc van Grootel — Fontys ICT, Interaction Design Research Group

---

*DVerse Communication Platform 2.0 · Fontys ICT · Interaction Design Research Group · 2026*
