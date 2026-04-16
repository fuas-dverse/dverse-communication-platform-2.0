from pydantic import BaseModel
from typing import Optional
from .bot import BotConfig


class RoomCreate(BaseModel):
    name: str
    description: str = ""
    server_id: Optional[str] = None


class Room(BaseModel):
    id: str
    name: str
    description: str
    server_id: Optional[str] = None
    created_by: str
    created_at: str
    bots: list[BotConfig] = []
