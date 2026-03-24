from pydantic import BaseModel


class MessageCreate(BaseModel):
    content: str


class Message(BaseModel):
    id: str
    room_id: str
    user_id: str
    username: str
    content: str
    is_bot: bool
    bot_id: str | None = None
    bot_triggered_by: str | None = None
    created_at: str
