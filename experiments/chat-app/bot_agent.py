#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "eclipse-zenoh~=0.11.0",
#   "anthropic",
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

import zenoh

parser = argparse.ArgumentParser(description="Zenoh Bot Agent")
parser.add_argument("--name",   default=os.environ.get("BOT_NAME", "mybot"),                help="Bot handle, must match @name in the room (env: BOT_NAME)")
parser.add_argument("--router", default=os.environ.get("ZENOH_ROUTER", "tcp/localhost:7447"), help="Zenoh router endpoint (env: ZENOH_ROUTER)")
parser.add_argument("--api-key",default=os.environ.get("ANTHROPIC_API_KEY", ""),             help="Anthropic API key (env: ANTHROPIC_API_KEY)")
args = parser.parse_args()

BOT_NAME = args.name
ZENOH_ROUTER = args.router
ANTHROPIC_API_KEY = args.api_key


# ── LLM call ─────────────────────────────────────────────────────────────────
# Swap this function for any LLM you want (OpenAI, Ollama, local model, etc.)

def call_llm(message: str, history: list[dict]) -> str:
    from anthropic import Anthropic
    client = Anthropic(api_key=ANTHROPIC_API_KEY)

    messages = history + [{"role": "user", "content": message}]

    response = client.messages.create(
        model="claude-haiku-4-5-20251001",
        max_tokens=1024,
        system=(
            f"You are @{BOT_NAME}, a helpful assistant in a group chat. "
            f"Only respond when you are mentioned with @{BOT_NAME}. "
            "Be concise and relevant."
        ),
        messages=messages,
    )
    return response.content[0].text


# ── Zenoh handler ─────────────────────────────────────────────────────────────

def on_request(sample, session):
    try:
        raw = bytes(sample.payload).decode("utf-8")
        data = json.loads(raw)

        request_id = data["request_id"]
        room_id = data["room_id"]
        message = data["message"]
        history = data.get("history", [])

        print(f"[{BOT_NAME}] Request in room {room_id[:8]}…: {message[:60]}")

        content = call_llm(message, history)

        response_payload = json.dumps({"request_id": request_id, "content": content})
        session.put(f"chat/response/{request_id}", response_payload)

        print(f"[{BOT_NAME}] Responded: {content[:60]}…")

    except Exception as exc:
        print(f"[{BOT_NAME}] Error handling request: {exc}")
        # Send error back so the chat doesn't hang on "thinking..."
        try:
            response_payload = json.dumps({
                "request_id": data.get("request_id", ""),
                "content": f"[Bot error: {exc}]",
            })
            session.put(f"chat/response/{data['request_id']}", response_payload)
        except Exception:
            pass


# ── Main ──────────────────────────────────────────────────────────────────────

def main():
    print(f"Starting bot @{BOT_NAME}")
    print(f"Connecting to Zenoh router at {ZENOH_ROUTER} ...")

    conf = zenoh.Config()
    conf.insert_json5("connect/endpoints", json.dumps([ZENOH_ROUTER]))
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
