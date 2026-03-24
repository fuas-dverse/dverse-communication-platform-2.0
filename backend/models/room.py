from pydantic import BaseModel
from .bot import BotConfig


class RoomCreate(BaseModel):
    name: str
    description: str = ""


class Room(BaseModel):
    id: str
    name: str
    description: str
    created_by: str
    created_at: str
    bots: list[BotConfig] = []
