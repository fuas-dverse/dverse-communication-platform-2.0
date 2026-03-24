import os
import re

import anthropic
import httpx

from ..models.bot import BotConfig, BotPersonality, BotProvider, PERSONALITY_PROMPTS

DEFAULT_CLAUDE_MODEL = "claude-haiku-4-5-20251001"
DEFAULT_LOCAL_URL = "http://localhost:11434/v1"
DEFAULT_LOCAL_MODEL = "deepseek-r1:1.5b"


def _build_system_prompt(bot: BotConfig) -> str:
    personality_key = BotPersonality(bot.personality)
    base = PERSONALITY_PROMPTS.get(personality_key, PERSONALITY_PROMPTS[BotPersonality.ASSISTANT])
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
    system_prompt = _build_system_prompt(bot)

    messages = [{"role": "system", "content": system_prompt}]
    messages += [{"role": e["role"], "content": e["content"]} for e in history]
    if not history or history[-1]["content"] != triggering_message:
        messages.append({"role": "user", "content": triggering_message})

    with httpx.Client(timeout=120.0) as client:
        resp = client.post(
            f"{base_url.rstrip('/')}/chat/completions",
            json={"model": model, "messages": messages},
        )
        resp.raise_for_status()
        data = resp.json()

    raw_text = data["choices"][0]["message"]["content"]
    return re.sub(r"<think>.*?</think>", "", raw_text, flags=re.DOTALL).strip()


def build_bot_response(bot: BotConfig, triggering_message: str, history: list[dict]) -> str:
    if bot.provider == BotProvider.CLAUDE:
        return call_claude(bot, triggering_message, history)
    elif bot.provider == BotProvider.LOCAL:
        return call_local(bot, triggering_message, history)
    raise ValueError(f"Unknown provider: {bot.provider}")
