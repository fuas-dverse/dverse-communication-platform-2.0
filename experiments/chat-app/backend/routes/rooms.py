import asyncio
import json
import uuid
from datetime import datetime
from typing import Optional

from fastapi import APIRouter, Depends, HTTPException, Request, status
from fastapi.responses import StreamingResponse

from ..auth import get_current_user
from ..db import get_db
from ..models.bot import BotConfig, BotConfigCreate, BotConfigUpdate
from ..models.room import Room, RoomCreate
from ..models.user import User
from ..services.sse import broker

GLOBAL_CHANNEL = "__global__"

router = APIRouter()


def _server_channel(server_id: str) -> str:
    return f"server:{server_id}"


def _get_bots_for_room(db, room_id: str) -> list[BotConfig]:
    rows = db.execute(
        """
        SELECT rb.*, u.username AS added_by_username
        FROM room_bots rb
        LEFT JOIN users u ON rb.added_by = u.id
        WHERE rb.room_id = ?
        ORDER BY rb.created_at ASC
        """,
        (room_id,),
    ).fetchall()
    bots = []
    for row in rows:
        data = dict(row)
        data["added_by"] = data.pop("added_by_username", None) or data.get("added_by")
        bots.append(BotConfig(**data))
    return bots


def _row_to_room(db, row) -> Room:
    bots = _get_bots_for_room(db, row["id"])
    data = dict(row)
    return Room(
        id=data["id"],
        name=data["name"],
        description=data.get("description") or "",
        server_id=data.get("server_id"),
        created_by=data["created_by"],
        created_at=data["created_at"],
        bots=bots,
    )


@router.get("/stream")
async def stream_rooms(request: Request, current_user: User = Depends(get_current_user)):
    async def event_generator():
        q = await broker.subscribe(GLOBAL_CHANNEL)
        try:
            while True:
                try:
                    data = await asyncio.wait_for(q.get(), timeout=15.0)
                    event_type = data.get("type", "message")
                    yield f"event: {event_type}\ndata: {json.dumps(data)}\n\n"
                except asyncio.TimeoutError:
                    yield "event: ping\ndata: \n\n"
        finally:
            broker.unsubscribe(GLOBAL_CHANNEL, q)

    return StreamingResponse(
        event_generator(),
        media_type="text/event-stream",
        headers={"Cache-Control": "no-cache", "X-Accel-Buffering": "no"},
    )


@router.get("/", response_model=list[Room])
def list_rooms(
    server_id: Optional[str] = None,
    current_user: User = Depends(get_current_user),
):
    db = get_db()
    if server_id:
        rows = db.execute(
            "SELECT * FROM rooms WHERE server_id = ? ORDER BY created_at ASC",
            (server_id,),
        ).fetchall()
    else:
        rows = db.execute("SELECT * FROM rooms ORDER BY created_at DESC").fetchall()
    return [_row_to_room(db, row) for row in rows]


@router.post("/", response_model=Room, status_code=status.HTTP_201_CREATED)
async def create_room(body: RoomCreate, current_user: User = Depends(get_current_user)):
    db = get_db()

    # Verify server membership if server_id provided
    if body.server_id:
        member_check = db.execute(
            "SELECT 1 FROM server_members WHERE server_id = ? AND user_id = ?",
            (body.server_id, current_user.id),
        ).fetchone()
        if not member_check:
            raise HTTPException(
                status_code=status.HTTP_403_FORBIDDEN,
                detail="You are not a member of this server",
            )

    existing = db.execute("SELECT id FROM rooms WHERE name = ?", (body.name,)).fetchone()
    if existing:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail="A room with that name already exists",
        )

    room_id = str(uuid.uuid4())
    created_at = datetime.utcnow().isoformat() + "Z"

    db.execute(
        "INSERT INTO rooms (id, name, description, created_by, created_at, server_id) VALUES (?, ?, ?, ?, ?, ?)",
        (room_id, body.name, body.description, current_user.id, created_at, body.server_id),
    )
    db.commit()

    room = Room(
        id=room_id,
        name=body.name,
        description=body.description,
        server_id=body.server_id,
        created_by=current_user.id,
        created_at=created_at,
        bots=[],
    )

    payload = {"type": "room_created", "room": room.model_dump()}
    # Broadcast to server channel (if server-scoped) and global
    if body.server_id:
        await broker.publish(_server_channel(body.server_id), payload)
    await broker.publish(GLOBAL_CHANNEL, payload)

    return room


@router.get("/{room_id}", response_model=Room)
def get_room(room_id: str, current_user: User = Depends(get_current_user)):
    db = get_db()
    row = db.execute("SELECT * FROM rooms WHERE id = ?", (room_id,)).fetchone()
    if not row:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Room not found")
    return _row_to_room(db, row)


@router.post("/{room_id}/bots", response_model=BotConfig, status_code=status.HTTP_201_CREATED)
def add_bot(
    room_id: str,
    body: BotConfigCreate,
    current_user: User = Depends(get_current_user),
):
    db = get_db()

    room = db.execute("SELECT * FROM rooms WHERE id = ?", (room_id,)).fetchone()
    if not room:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Room not found")

    if room["created_by"] != current_user.id:
        raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail="Only the room creator can add bots")

    if body.provider == "zenoh":
        if not body.token:
            raise HTTPException(status_code=status.HTTP_422_UNPROCESSABLE_ENTITY, detail="A token is required to add a Zenoh bot")
        from ..services.zenoh_bridge import zenoh_bridge
        if not zenoh_bridge.verify_bot_token(body.name, body.token):
            raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail="Invalid token — the bot owner must share the correct token with you")

    existing_bot = db.execute(
        "SELECT id FROM room_bots WHERE room_id = ? AND name = ?",
        (room_id, body.name),
    ).fetchone()
    if existing_bot:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail=f"A bot named '{body.name}' already exists in this room",
        )

    bot_id = str(uuid.uuid4())
    created_at = datetime.utcnow().isoformat() + "Z"

    db.execute(
        "INSERT INTO room_bots (id, room_id, name, provider, personality, model, system_prompt, added_by, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        (bot_id, room_id, body.name, body.provider, body.personality, body.model, body.system_prompt, current_user.id, created_at),
    )
    db.commit()

    return BotConfig(
        id=bot_id,
        room_id=room_id,
        name=body.name,
        provider=body.provider,
        personality=body.personality,
        model=body.model,
        system_prompt=body.system_prompt,
        added_by=current_user.username,
        created_at=created_at,
    )


@router.patch("/{room_id}/bots/{bot_id}", response_model=BotConfig)
def update_bot(
    room_id: str,
    bot_id: str,
    body: BotConfigUpdate,
    current_user: User = Depends(get_current_user),
):
    db = get_db()

    room = db.execute("SELECT * FROM rooms WHERE id = ?", (room_id,)).fetchone()
    if not room:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Room not found")

    if room["created_by"] != current_user.id:
        raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail="Only the room creator can update bots")

    bot_row = db.execute(
        "SELECT * FROM room_bots WHERE id = ? AND room_id = ?", (bot_id, room_id)
    ).fetchone()
    if not bot_row:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Bot not found")

    updates = body.model_dump(exclude_unset=True)
    if not updates:
        return BotConfig(**dict(bot_row))

    set_clauses = ", ".join(f"{k} = ?" for k in updates.keys())
    values = list(updates.values()) + [bot_id]

    db.execute(f"UPDATE room_bots SET {set_clauses} WHERE id = ?", values)
    db.commit()

    updated_row = db.execute("SELECT * FROM room_bots WHERE id = ?", (bot_id,)).fetchone()
    return BotConfig(**dict(updated_row))


@router.delete("/{room_id}/bots/{bot_id}", status_code=status.HTTP_200_OK)
def delete_bot(
    room_id: str,
    bot_id: str,
    current_user: User = Depends(get_current_user),
):
    db = get_db()

    room = db.execute("SELECT * FROM rooms WHERE id = ?", (room_id,)).fetchone()
    if not room:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Room not found")

    if room["created_by"] != current_user.id:
        raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail="Only the room creator can delete bots")

    bot_row = db.execute(
        "SELECT id FROM room_bots WHERE id = ? AND room_id = ?", (bot_id, room_id)
    ).fetchone()
    if not bot_row:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Bot not found")

    # Null out bot_id on messages before delete — SQLite FK prevents delete otherwise
    db.execute("UPDATE messages SET bot_id = NULL WHERE bot_id = ?", (bot_id,))
    db.execute("DELETE FROM room_bots WHERE id = ?", (bot_id,))
    db.commit()

    return {}
