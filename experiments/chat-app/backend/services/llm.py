import os
import re
import json as json_lib
from datetime import datetime, timezone

import anthropic
import httpx

from ..models.bot import BotConfig, BotPersonality, BotProvider, PERSONALITY_PROMPTS

DEFAULT_CLAUDE_MODEL = "claude-haiku-4-5-20251001"
DEFAULT_LOCAL_URL = "http://localhost:11434"
DEFAULT_LOCAL_MODEL = "deepseek-r1:1.5b"
DEFAULT_LOCAL_TIMEOUT_SECONDS = 240.0


def _messages_to_prompt(messages: list[dict]) -> str:
    lines: list[str] = []
    for msg in messages:
        role = msg.get("role", "user")
        content = msg.get("content", "")
        if role == "system":
            lines.append(f"System: {content}")
        elif role == "assistant":
            lines.append(f"Assistant: {content}")
        else:
            lines.append(f"User: {content}")
    lines.append("Assistant:")
    return "\n\n".join(lines)


def _build_system_prompt(bot: BotConfig) -> str:
    personality_key = BotPersonality(bot.personality)
    base = PERSONALITY_PROMPTS.get(personality_key, PERSONALITY_PROMPTS[BotPersonality.ASSISTANT])
    now = datetime.now(timezone.utc)
    today_label = f"{now.date().isoformat()} ({now.strftime('%A')}, UTC)"
    return (
        f"{base}\n\n"
        f"Your name is @{bot.name}. "
        f"Only respond when directly mentioned with @{bot.name}. "
        "Keep responses concise and relevant to the conversation."
    )


def call_claude(bot: BotConfig, triggering_message: str, history: list[dict]) -> str:
    api_key = os.environ.get("ANTHROPIC_API_KEY", "")
    model = bot.model or DEFAULT_CLAUDE_MODEL
    system_prompt = _build_system_prompt(bot)

    client = anthropic.Anthropic(api_key=api_key)

    messages = [{"role": e["role"], "content": e["content"]} for e in history]
    if not messages or messages[-1]["content"] != triggering_message:
        messages.append({"role": "user", "content": triggering_message})

    response = client.messages.create(
        model=model,
        max_tokens=1024,
        system=system_prompt,
        messages=messages,
    )
    return response.content[0].text


def call_local(bot: BotConfig, triggering_message: str, history: list[dict]) -> str:
    base_url = os.environ.get("LOCAL_LLM_URL", DEFAULT_LOCAL_URL)
    model = bot.model or os.environ.get("LOCAL_LLM_MODEL", DEFAULT_LOCAL_MODEL)
    timeout_seconds = float(
        os.environ.get("LOCAL_LLM_TIMEOUT_SECONDS", str(DEFAULT_LOCAL_TIMEOUT_SECONDS))
    )
    system_prompt = _build_system_prompt(bot)

    messages = [{"role": "system", "content": system_prompt}]
    messages += [{"role": e["role"], "content": e["content"]} for e in history]
    if not history or history[-1]["content"] != triggering_message:
        messages.append({"role": "user", "content": triggering_message})

    normalized_base_url = base_url.rstrip("/")
    candidates: list[tuple[str, str]] = []
    if normalized_base_url.endswith("/v1"):
        root_url = normalized_base_url[:-3].rstrip("/")
        candidates.append(("openai", f"{normalized_base_url}/chat/completions"))
        candidates.append(("ollama", f"{root_url}/api/chat"))
        candidates.append(("ollama_generate", f"{root_url}/api/generate"))
    else:
        candidates.append(("ollama", f"{normalized_base_url}/api/chat"))
        candidates.append(("ollama_generate", f"{normalized_base_url}/api/generate"))
        candidates.append(("openai", f"{normalized_base_url}/v1/chat/completions"))

    with httpx.Client(timeout=timeout_seconds) as client:
        last_error: str | None = None
        for api_style, url in candidates:
            if api_style == "openai":
                payload = {"model": model, "messages": messages}
            elif api_style == "ollama":
                payload = {"model": model, "messages": messages}
                payload["stream"] = False
            else:
                payload = {
                    "model": model,
                    "prompt": _messages_to_prompt(messages),
                    "stream": False,
                }

            resp = client.post(url, json=payload)
            if resp.status_code == 404:
                details = ""
                try:
                    payload = resp.json()
                    if isinstance(payload, dict):
                        details = str(payload.get("error", "")).strip()
                except (json_lib.JSONDecodeError, ValueError):
                    details = resp.text.strip()

                if "not found" in details.lower() and "model" in details.lower():
                    raise RuntimeError(
                        f"Local model '{model}' is not installed. "
                        f"Run: ollama pull {model}"
                    )

                last_error = f"404 from {url}{': ' + details if details else ''}"
                continue

            resp.raise_for_status()
            data = resp.json()

            if api_style == "openai":
                raw_text = data["choices"][0]["message"]["content"]
            elif api_style == "ollama":
                raw_text = data.get("message", {}).get("content", "")
            else:
                raw_text = data.get("response", "")

            # Some local endpoints/models return text in alternate fields.
            if not raw_text:
                raw_text = (
                    data.get("response")
                    or data.get("content")
                    or data.get("output_text")
                    or ""
                )

            if raw_text:
                cleaned = re.sub(r"<think>.*?</think>", "", raw_text, flags=re.DOTALL).strip()
                if cleaned:
                    return cleaned

            last_error = f"empty response from {url}"
            continue

    attempted = ", ".join(url for _, url in candidates)
    detail = f" Last error: {last_error}" if last_error else ""
    raise RuntimeError(f"Failed to reach local LLM endpoint. Tried: {attempted}.{detail}")


def build_bot_response(bot: BotConfig, triggering_message: str, history: list[dict]) -> str:
    if bot.provider == BotProvider.CLAUDE:
        return call_claude(bot, triggering_message, history)
    elif bot.provider == BotProvider.LOCAL:
        return call_local(bot, triggering_message, history)
    raise ValueError(f"Unknown provider: {bot.provider}")
