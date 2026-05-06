from pydantic import BaseModel


class ServerCreate(BaseModel):
    name: str
    description: str = ""


class ServerMember(BaseModel):
    user_id: str
    username: str
    joined_at: str
    is_online: bool = False
    last_seen: str | None = None


class Server(BaseModel):
    id: str
    name: str
    description: str
    created_by: str
    created_at: str
    member_count: int = 0
    invite_code: str | None = None
