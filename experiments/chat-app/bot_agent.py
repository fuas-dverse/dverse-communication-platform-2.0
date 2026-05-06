#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "eclipse-zenoh~=1.8.0",
#   "httpx",
# ]
# ///
"""
Zenoh Bot Agent — run this on any machine to add your AI to the chat.

Configure with env vars:
  BOT_NAME=mybot                   # must match the @name in the room
  ZENOH_ROUTER=tcp/localhost:7447  # where the Zenoh router is
  ANTHROPIC_API_KEY=sk-ant-...     # or swap call_llm() for any other model

Three ways to run — pick one:
  1. Compiled binary (no Python needed):
       ./dist/bot_agent            (Linux/macOS — run ./scripts/build.sh first)
       dist\\bot_agent.exe         (Windows    — run scripts\\build.bat first)

  2. Via uv (no Python needed, uv bundles its own):
       ./bot_agent.py              (Linux/macOS)
       uv run bot_agent.py         (Windows / anywhere)

  3. Plain Python:
       pip install eclipse-zenoh~=0.11.0 anthropic
       python bot_agent.py
"""

import argparse
import json
import os
import signal
import sys
import threading

import zenoh

parser = argparse.ArgumentParser(description="Zenoh Bot Agent")
parser.add_argument("--name",       default=os.environ.get("BOT_NAME", "mybot"),                   help="Bot handle, must match @name in the room (env: BOT_NAME)")
parser.add_argument("--router",     default=os.environ.get("ZENOH_ROUTER", "tcp/localhost:7447"),   help="Zenoh router endpoint (env: ZENOH_ROUTER)")
parser.add_argument("--ollama-url", default=os.environ.get("OLLAMA_URL", "http://localhost:11434"), help="Ollama base URL (env: OLLAMA_URL)")
parser.add_argument("--model",      default=os.environ.get("OLLAMA_MODEL", "deepseek-r1:1.5b"),    help="Ollama model name (env: OLLAMA_MODEL)")
args = parser.parse_args()

BOT_NAME = args.name
ZENOH_ROUTER = args.router
OLLAMA_URL = args.ollama_url
OLLAMA_MODEL = args.model


# ── LLM call ─────────────────────────────────────────────────────────────────
# Swap this function for any LLM you want (OpenAI, Ollama, local model, etc.)

def call_llm(message: str, history: list[dict]) -> str:
    import httpx

    messages = [{"role": "system", "content": (
        f"You are @{BOT_NAME}, a helpful assistant in a group chat. "
        f"Only respond when you are mentioned with @{BOT_NAME}. "
        "Be concise and relevant."
    )}] + history + [{"role": "user", "content": message}]

    response = httpx.post(
        f"{OLLAMA_URL}/api/chat",
        json={"model": OLLAMA_MODEL, "messages": messages, "stream": False},
        timeout=120.0,
    )
    response.raise_for_status()
    return response.json()["message"]["content"]


# ── Zenoh handler ─────────────────────────────────────────────────────────────

def handle_request(data, session):
    request_id = data["request_id"]
    try:
        content = call_llm(data["message"], data.get("history", []))
        response_payload = json.dumps({"request_id": request_id, "content": content})
        session.put(f"chat/response/{request_id}", response_payload.encode())
        print(f"[{BOT_NAME}] Responded: {content[:60]}…")
    except Exception as exc:
        print(f"[{BOT_NAME}] Error: {exc}")
        err_payload = json.dumps({"request_id": request_id, "content": f"[Bot error: {exc}]"})
        session.put(f"chat/response/{request_id}", err_payload.encode())


def on_request(sample, session):
    try:
        data = json.loads(bytes(sample.payload.to_bytes()).decode("utf-8"))
        print(f"[{BOT_NAME}] Request in room {data['room_id'][:8]}…: {data['message'][:60]}")
        threading.Thread(target=handle_request, args=(data, session), daemon=True).start()
    except Exception as exc:
        print(f"[{BOT_NAME}] Failed to parse request: {exc}")


# ── Main ──────────────────────────────────────────────────────────────────────

def main():
    print(f"Starting bot @{BOT_NAME}")
    print(f"Connecting to Zenoh router at {ZENOH_ROUTER} ...")

    conf = zenoh.Config.from_json5(json.dumps({
        "connect": {"endpoints": [ZENOH_ROUTER]}
    }))
    session = zenoh.open(conf)

    # Subscribe to all requests for this bot across all rooms
    key_expr = f"chat/*/request/{BOT_NAME}"
    sub = session.declare_subscriber(
        key_expr,
        lambda sample: on_request(sample, session),
    )

    print(f"@{BOT_NAME} is online. Listening on '{key_expr}'")
    print("Press Ctrl+C to stop.\n")

    def shutdown(sig, frame):
        print(f"\nShutting down @{BOT_NAME}...")
        sub.undeclare()
        session.close()
        sys.exit(0)

    signal.signal(signal.SIGINT, shutdown)
    signal.signal(signal.SIGTERM, shutdown)
    signal.pause()


if __name__ == "__main__":
    main()
