import asyncio
import json
import uuid
from datetime import datetime

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


def _get_bots_for_room(db, room_id: str) -> list[BotConfig]:
    rows = db.execute(
        "SELECT * FROM room_bots WHERE room_id = ? ORDER BY created_at ASC",
        (room_id,),
    ).fetchall()
    return [BotConfig(**dict(row)) for row in rows]


def _row_to_room(db, row) -> Room:
    bots = _get_bots_for_room(db, row["id"])
    return Room(
        id=row["id"],
        name=row["name"],
        description=row["description"] or "",
        created_by=row["created_by"],
        created_at=row["created_at"],
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
def list_rooms(current_user: User = Depends(get_current_user)):
    db = get_db()
    rows = db.execute(
        "SELECT * FROM rooms ORDER BY created_at DESC"
    ).fetchall()
    return [_row_to_room(db, row) for row in rows]


@router.post("/", response_model=Room, status_code=status.HTTP_201_CREATED)
async def create_room(body: RoomCreate, current_user: User = Depends(get_current_user)):
    db = get_db()

    existing = db.execute("SELECT id FROM rooms WHERE name = ?", (body.name,)).fetchone()
    if existing:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail="A room with that name already exists",
        )

    room_id = str(uuid.uuid4())
    created_at = datetime.utcnow().isoformat() + "Z"

    db.execute(
        "INSERT INTO rooms (id, name, description, created_by, created_at) VALUES (?, ?, ?, ?, ?)",
        (room_id, body.name, body.description, current_user.id, created_at),
    )
    db.commit()

    room = Room(
        id=room_id,
        name=body.name,
        description=body.description,
        created_by=current_user.id,
        created_at=created_at,
        bots=[],
    )
    await broker.publish(GLOBAL_CHANNEL, {"type": "room_created", "room": room.model_dump()})
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
        "INSERT INTO room_bots (id, room_id, name, provider, personality, model, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        (bot_id, room_id, body.name, body.provider, body.personality, body.model, created_at),
    )
    db.commit()

    return BotConfig(
        id=bot_id,
        room_id=room_id,
        name=body.name,
        provider=body.provider,
        personality=body.personality,
        model=body.model,
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

    updated_row = db.execute(
        "SELECT * FROM room_bots WHERE id = ?", (bot_id,)
    ).fetchone()

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

    db.execute("DELETE FROM room_bots WHERE id = ?", (bot_id,))
    db.commit()

    return {}
