import asyncio
import json
import uuid
from datetime import datetime, timedelta, timezone

from fastapi import APIRouter, Depends, HTTPException, Request, status
from fastapi.responses import StreamingResponse

from ..auth import get_current_user
from ..db import get_db
from ..models.server import Server, ServerCreate, ServerMember
from ..models.user import User
from ..services.sse import broker

router = APIRouter()

_presence_connections: dict[str, int] = {}
_presence_last_seen: dict[str, datetime] = {}
_presence_lock = asyncio.Lock()
PRESENCE_TTL_SECONDS = 45


def _server_channel(server_id: str) -> str:
    return f"server:{server_id}"


def _is_member_online(is_online: bool, last_seen: str | None) -> bool:
    if not is_online or not last_seen:
        return False
    try:
        seen_at = datetime.fromisoformat(last_seen.replace("Z", "+00:00"))
    except ValueError:
        return False
    now = datetime.now(timezone.utc)
    return now - seen_at <= timedelta(seconds=PRESENCE_TTL_SECONDS)


def _utc_now() -> datetime:
    return datetime.now(timezone.utc)


def _utc_iso_now() -> str:
    return datetime.utcnow().isoformat() + "Z"


def _list_user_server_ids(user_id: str) -> list[str]:
    db = get_db()
    rows = db.execute(
        "SELECT server_id FROM server_members WHERE user_id = ?",
        (user_id,),
    ).fetchall()
    return [row["server_id"] for row in rows]


async def _broadcast_user_presence(user_id: str, is_online: bool, last_seen: str | None = None) -> None:
    payload: dict = {"user_id": user_id}
    if is_online:
        payload["type"] = "member_online"
    else:
        payload["type"] = "member_offline"
        payload["last_seen"] = last_seen

    for server_id in _list_user_server_ids(user_id):
        await broker.publish(_server_channel(server_id), payload)


async def _touch_member_presence(server_id: str, user: User) -> None:
    async with _presence_lock:
        _presence_last_seen[user.id] = _utc_now()


async def _mark_member_online(server_id: str, user: User) -> None:
    key = user.id
    should_broadcast = False

    async with _presence_lock:
        prev = _presence_connections.get(key, 0)
        _presence_connections[key] = prev + 1
        _presence_last_seen[key] = _utc_now()
        should_broadcast = prev == 0

    if not should_broadcast:
        return

    db = get_db()
    now = _utc_iso_now()
    db.execute(
        "UPDATE server_members SET is_online = 1, last_seen = ? WHERE user_id = ?",
        (now, user.id),
    )
    db.commit()

    await _broadcast_user_presence(user.id, is_online=True)


async def _mark_member_offline_if_last_connection(server_id: str, user: User) -> None:
    key = user.id
    should_broadcast = False

    async with _presence_lock:
        prev = _presence_connections.get(key, 0)
        if prev <= 1:
            _presence_connections.pop(key, None)
            _presence_last_seen[key] = _utc_now()
            should_broadcast = prev > 0
        else:
            _presence_connections[key] = prev - 1
            _presence_last_seen[key] = _utc_now()

    if not should_broadcast:
        return

    db = get_db()
    now = _utc_iso_now()
    db.execute(
        "UPDATE server_members SET is_online = 0, last_seen = ? WHERE user_id = ?",
        (now, user.id),
    )
    db.commit()

    await _broadcast_user_presence(user.id, is_online=False, last_seen=now)


def _row_to_server(db, row) -> Server:
    member_count = db.execute(
        "SELECT COUNT(*) FROM server_members WHERE server_id = ?", (row["id"],)
    ).fetchone()[0]
    return Server(
        id=row["id"],
        name=row["name"],
        description=row["description"] or "",
        created_by=row["created_by"],
        created_at=row["created_at"],
        member_count=member_count,
        invite_code=row["invite_code"] if "invite_code" in row.keys() else None,
    )


@router.get("/", response_model=list[Server])
def list_servers(current_user: User = Depends(get_current_user)):
    """List all servers the current user is a member of."""
    db = get_db()
    rows = db.execute(
        """
        SELECT s.* FROM servers s
        JOIN server_members sm ON sm.server_id = s.id
        WHERE sm.user_id = ?
        ORDER BY s.created_at ASC
        """,
        (current_user.id,),
    ).fetchall()
    return [_row_to_server(db, row) for row in rows]


@router.post("/", response_model=Server, status_code=status.HTTP_201_CREATED)
async def create_server(body: ServerCreate, current_user: User = Depends(get_current_user)):
    db = get_db()

    existing = db.execute("SELECT id FROM servers WHERE name = ?", (body.name,)).fetchone()
    if existing:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail="A server with that name already exists",
        )

    server_id = str(uuid.uuid4())
    created_at = datetime.utcnow().isoformat() + "Z"

    db.execute(
        "INSERT INTO servers (id, name, description, created_by, created_at) VALUES (?, ?, ?, ?, ?)",
        (server_id, body.name, body.description, current_user.id, created_at),
    )
    # Auto-join the creator
    db.execute(
        "INSERT INTO server_members (server_id, user_id, username, joined_at) VALUES (?, ?, ?, ?)",
        (server_id, current_user.id, current_user.username, created_at),
    )
    db.commit()

    server = Server(
        id=server_id,
        name=body.name,
        description=body.description,
        created_by=current_user.id,
        created_at=created_at,
        member_count=1,
    )
    return server


@router.post("/{server_id}/join", response_model=Server)
async def join_server(server_id: str, current_user: User = Depends(get_current_user)):
    db = get_db()

    row = db.execute("SELECT * FROM servers WHERE id = ?", (server_id,)).fetchone()
    if not row:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Server not found")

    # Idempotent — already a member is fine
    existing = db.execute(
        "SELECT 1 FROM server_members WHERE server_id = ? AND user_id = ?",
        (server_id, current_user.id),
    ).fetchone()
    if not existing:
        now = datetime.utcnow().isoformat() + "Z"
        db.execute(
            "INSERT INTO server_members (server_id, user_id, username, joined_at, is_online, last_seen) VALUES (?, ?, ?, ?, ?, ?)",
            (server_id, current_user.id, current_user.username, now, 1, now),
        )
        db.commit()

        member = ServerMember(
            user_id=current_user.id,
            username=current_user.username,
            joined_at=now,
            is_online=True,
            last_seen=now,
        )
        await broker.publish(
            _server_channel(server_id),
            {"type": "member_joined", "member": member.model_dump()},
        )
    else:
        # Update last_seen if already member
        now = datetime.utcnow().isoformat() + "Z"
        db.execute(
            "UPDATE server_members SET is_online = 1, last_seen = ? WHERE server_id = ? AND user_id = ?",
            (now, server_id, current_user.id),
        )
        db.commit()

        # Broadcast presence update
        await broker.publish(
            _server_channel(server_id),
            {"type": "member_online", "user_id": current_user.id},
        )

    return _row_to_server(db, row)


@router.delete("/{server_id}/leave", status_code=status.HTTP_200_OK)
async def leave_server(server_id: str, current_user: User = Depends(get_current_user)):
    db = get_db()

    row = db.execute("SELECT * FROM servers WHERE id = ?", (server_id,)).fetchone()
    if not row:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Server not found")

    if row["created_by"] == current_user.id:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Server owner cannot leave — delete the server instead",
        )

    # Mark as offline instead of deleting
    now = datetime.utcnow().isoformat() + "Z"
    db.execute(
        "UPDATE server_members SET is_online = 0, last_seen = ? WHERE server_id = ? AND user_id = ?",
        (now, server_id, current_user.id),
    )
    db.commit()

    # Broadcast presence update
    await broker.publish(
        _server_channel(server_id),
        {"type": "member_offline", "user_id": current_user.id, "last_seen": now},
    )
    return {}


@router.post("/{server_id}/invite", response_model=Server)
def generate_invite(server_id: str, current_user: User = Depends(get_current_user)):
    """Generate (or regenerate) an invite code for a server. Owner only."""
    db = get_db()

    row = db.execute("SELECT * FROM servers WHERE id = ?", (server_id,)).fetchone()
    if not row:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Server not found")
    if row["created_by"] != current_user.id:
        raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail="Only the server owner can manage invite codes")

    code = uuid.uuid4().hex[:8].upper()
    db.execute("UPDATE servers SET invite_code = ? WHERE id = ?", (code, server_id))
    db.commit()

    return _row_to_server(db, db.execute("SELECT * FROM servers WHERE id = ?", (server_id,)).fetchone())


@router.post("/join/{invite_code}", response_model=Server)
async def join_by_invite(invite_code: str, current_user: User = Depends(get_current_user)):
    """Join a server using an invite code."""
    db = get_db()

    row = db.execute("SELECT * FROM servers WHERE invite_code = ?", (invite_code.upper(),)).fetchone()
    if not row:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Invalid invite code")

    existing = db.execute(
        "SELECT 1 FROM server_members WHERE server_id = ? AND user_id = ?",
        (row["id"], current_user.id),
    ).fetchone()
    if not existing:
        now = datetime.utcnow().isoformat() + "Z"
        db.execute(
            "INSERT INTO server_members (server_id, user_id, username, joined_at, is_online, last_seen) VALUES (?, ?, ?, ?, ?, ?)",
            (row["id"], current_user.id, current_user.username, now, 1, now),
        )
        db.commit()

        member = ServerMember(
            user_id=current_user.id,
            username=current_user.username,
            joined_at=now,
            is_online=True,
            last_seen=now,
        )
        await broker.publish(
            _server_channel(row["id"]),
            {"type": "member_joined", "member": member.model_dump()},
        )
    else:
        # Update last_seen if already member
        now = datetime.utcnow().isoformat() + "Z"
        db.execute(
            "UPDATE server_members SET is_online = 1, last_seen = ? WHERE server_id = ? AND user_id = ?",
            (now, row["id"], current_user.id),
        )
        db.commit()

        # Broadcast presence update
        await broker.publish(
            _server_channel(row["id"]),
            {"type": "member_online", "user_id": current_user.id},
        )

    return _row_to_server(db, db.execute("SELECT * FROM servers WHERE id = ?", (row["id"],)).fetchone())


@router.get("/{server_id}/members", response_model=list[ServerMember])
async def get_members(server_id: str, current_user: User = Depends(get_current_user)):
    db = get_db()

    # Verify server exists and requester is a member
    member_check = db.execute(
        "SELECT 1 FROM server_members WHERE server_id = ? AND user_id = ?",
        (server_id, current_user.id),
    ).fetchone()
    if not member_check:
        raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail="Not a member of this server")

    rows = db.execute(
        "SELECT user_id, username, joined_at, COALESCE(is_online, 0) as is_online, last_seen FROM server_members WHERE server_id = ? ORDER BY joined_at ASC",
        (server_id,),
    ).fetchall()

    now = _utc_now()
    async with _presence_lock:
        global_online = {
            user_id
            for user_id, count in _presence_connections.items()
            if count > 0 and (now - _presence_last_seen.get(user_id, now)) <= timedelta(seconds=PRESENCE_TTL_SECONDS)
        }

    members: list[ServerMember] = []
    for row in rows:
        row_dict = dict(row)
        if row_dict["user_id"] in global_online:
            row_dict["is_online"] = True
        else:
            row_dict["is_online"] = _is_member_online(
                bool(row_dict.get("is_online", 0)),
                row_dict.get("last_seen"),
            )
        members.append(ServerMember(**row_dict))
    return members


@router.get("/{server_id}/stream")
async def stream_server(
    server_id: str,
    request: Request,
    current_user: User = Depends(get_current_user),
):
    db = get_db()

    # Verify membership
    member_check = db.execute(
        "SELECT 1 FROM server_members WHERE server_id = ? AND user_id = ?",
        (server_id, current_user.id),
    ).fetchone()
    if not member_check:
        raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail="Not a member of this server")

    async def event_generator():
        await _mark_member_online(server_id, current_user)
        q = await broker.subscribe(_server_channel(server_id))
        try:
            while True:
                if await request.is_disconnected():
                    break
                try:
                    data = await asyncio.wait_for(q.get(), timeout=15.0)
                    yield f"data: {json.dumps(data)}\n\n"
                except asyncio.TimeoutError:
                    if await request.is_disconnected():
                        break
                    await _touch_member_presence(server_id, current_user)
                    yield "event: ping\ndata: \n\n"
        finally:
            broker.unsubscribe(_server_channel(server_id), q)
            await _mark_member_offline_if_last_connection(server_id, current_user)

    return StreamingResponse(
        event_generator(),
        media_type="text/event-stream",
        headers={"Cache-Control": "no-cache", "X-Accel-Buffering": "no"},
    )
