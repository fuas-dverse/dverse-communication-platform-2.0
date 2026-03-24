from enum import Enum
from pydantic import BaseModel, ConfigDict


class BotProvider(str, Enum):
    CLAUDE = "claude"
    LOCAL = "local"
    ZENOH = "zenoh"


class BotPersonality(str, Enum):
    ASSISTANT = "assistant"
    CODER = "coder"
    CREATIVE = "creative"
    ANALYST = "analyst"


PERSONALITY_PROMPTS = {
    BotPersonality.ASSISTANT: "You are a helpful, concise assistant in a group chat.",
    BotPersonality.CODER: "You are an expert software engineer. Favor code and technical precision.",
    BotPersonality.CREATIVE: "You are a creative thinker and storyteller. Be imaginative.",
    BotPersonality.ANALYST: "You are a sharp analyst. Be data-driven and critical.",
}


class BotConfigCreate(BaseModel):
    name: str
    provider: BotProvider
    personality: BotPersonality = BotPersonality.ASSISTANT
    model: str | None = None


class BotConfigUpdate(BaseModel):
    provider: BotProvider | None = None
    personality: BotPersonality | None = None
    model: str | None = None


class BotConfig(BotConfigCreate):
    id: str
    room_id: str
    created_at: str

    model_config = ConfigDict(from_attributes=True)
