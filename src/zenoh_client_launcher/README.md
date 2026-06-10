# Zenoh Client Launcher

Tauri desktop app for connecting to a Zenoh network and managing bot agents.

## Structure

```
zenoh-client-launcher/
├── src-tauri/          Rust backend (Tauri 2)
│   ├── src/lib.rs      Tauri commands: start_bot, stop_bot, get_bot_statuses
│   └── tauri.conf.json App config
└── frontend/           React + Vite + Tailwind
    └── src/
        ├── App.tsx
        ├── components/
        │   ├── ConnectTab.tsx   Tab 1: connect to Zenoh router
        │   ├── BotsTab.tsx      Tab 2: manage & launch bots
        │   ├── BotCard.tsx      Bot status card
        │   └── BotForm.tsx      Add/edit bot modal
        └── types.ts
```

## Dev

```bash
cd frontend && npm install
cd ..
cargo tauri dev
```

## Build

```bash
cargo tauri build
```

## How it works

1. **Network tab** — enter Zenoh router address + username, click "Request Access"
   - Admin sees request in Router Admin Dashboard (separate app)
   - Admin approves → Avahi assigns `username.chat.local` DNS
   - Status updates to "Connected"

2. **Bots tab** — add bots with name, personality, system prompt, LLM config
   - Click "Start" → spawns `bot_agent.py` as a child process
   - Bot connects to the Zenoh router and starts heartbeating
   - Bot token shown — share with users who want to add the bot to a room

## bot_agent.py location

Place `bot_agent.py` (from `experiments/chat-app/`) next to the launcher binary, or on PATH as `bot_agent`.
