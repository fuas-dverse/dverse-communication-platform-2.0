import asyncio
import json
import os
import re
import uuid
from datetime import datetime
from typing import Optional

from fastapi import APIRouter, Depends, HTTPException, Query, Request, status
from fastapi.responses import StreamingResponse

from ..auth import get_current_user
from ..db import get_db
from ..models.bot import BotConfig
from ..models.message import Message, MessageCreate
from ..models.user import User
from ..services.llm import build_bot_response
from ..services.sse import broker

router = APIRouter()
BOT_HISTORY_LIMIT = int(os.environ.get("BOT_HISTORY_LIMIT", "40"))
BOT_DEBUG_CONTEXT = os.environ.get("BOT_DEBUG_CONTEXT", "false").lower() in {
    "1",
    "true",
    "yes",
    "on",
}


def _row_to_message(row) -> Message:
    return Message(
        id=row["id"],
        room_id=row["room_id"],
        user_id=row["user_id"],
        username=row["username"],
        content=row["content"],
        is_bot=bool(row["is_bot"]),
        bot_id=row["bot_id"],
        bot_triggered_by=row["bot_triggered_by"],
        created_at=row["created_at"],
    )


def _fetch_messages(db, room_id: str, after: Optional[str] = None) -> list[Message]:
    if after:
        rows = db.execute(
            """
            SELECT m.*, u.username
            FROM messages m
            JOIN users u ON m.user_id = u.id
            WHERE m.room_id = ? AND m.created_at > ?
            ORDER BY m.created_at ASC
            LIMIT 100
            """,
            (room_id, after),
        ).fetchall()
    else:
        rows = db.execute(
            """
            SELECT m.*, u.username
            FROM messages m
            JOIN users u ON m.user_id = u.id
            WHERE m.room_id = ?
            ORDER BY m.created_at ASC
            LIMIT 100
            """,
            (room_id,),
        ).fetchall()
    return [_row_to_message(row) for row in rows]


@router.get("/{room_id}/messages", response_model=list[Message])
def get_messages(
    room_id: str,
    after: Optional[str] = Query(default=None),
    current_user: User = Depends(get_current_user),
):
    db = get_db()

    room = db.execute("SELECT id FROM rooms WHERE id = ?", (room_id,)).fetchone()
    if not room:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Room not found")

    return _fetch_messages(db, room_id, after)


@router.post("/{room_id}/messages", response_model=Message, status_code=status.HTTP_201_CREATED)
async def post_message(
    room_id: str,
    body: MessageCreate,
    current_user: User = Depends(get_current_user),
):
    db = get_db()

    room = db.execute("SELECT id FROM rooms WHERE id = ?", (room_id,)).fetchone()
    if not room:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Room not found")

    # Insert the user message
    msg_id = str(uuid.uuid4())
    created_at = datetime.utcnow().isoformat() + "Z"

    db.execute(
        """
        INSERT INTO messages (id, room_id, user_id, content, is_bot, bot_id, bot_triggered_by, created_at)
        VALUES (?, ?, ?, ?, 0, NULL, NULL, ?)
        """,
        (msg_id, room_id, current_user.id, body.content, created_at),
    )
    db.commit()

    user_message = Message(
        id=msg_id,
        room_id=room_id,
        user_id=current_user.id,
        username=current_user.username,
        content=body.content,
        is_bot=False,
        bot_id=None,
        bot_triggered_by=None,
        created_at=created_at,
    )

    # Emit SSE for user message
    await broker.publish(room_id, {"type": "message", "message": user_message.model_dump()})

    # Check if message mentions a bot (@botname anywhere in the message)
    content = body.content.strip()
    triggered_bot: Optional[BotConfig] = None

    mention_match = re.search(r"(?<!\w)@([a-zA-Z0-9-]+)\b", content)
    if mention_match:
        mention = mention_match.group(1).lower()
        bot_row = db.execute(
            "SELECT * FROM room_bots WHERE room_id = ? AND LOWER(name) = ?",
            (room_id, mention),
        ).fetchone()
        if bot_row:
            triggered_bot = BotConfig(**dict(bot_row))

    if triggered_bot is not None:
        # Insert a placeholder "thinking..." bot message
        placeholder_id = str(uuid.uuid4())
        placeholder_at = datetime.utcnow().isoformat() + "Z"

        db.execute(
            """
            INSERT INTO messages (id, room_id, user_id, content, is_bot, bot_id, bot_triggered_by, created_at)
            VALUES (?, ?, ?, ?, 1, ?, ?, ?)
            """,
            (
                placeholder_id,
                room_id,
                current_user.id,
                "thinking...",
                triggered_bot.id,
                current_user.id,
                placeholder_at,
            ),
        )
        db.commit()

        # Fetch bot's username (bots use their name as username label)
        bot_placeholder_message = Message(
            id=placeholder_id,
            room_id=room_id,
            user_id=current_user.id,
            username=f"@{triggered_bot.name}",
            content="thinking...",
            is_bot=True,
            bot_id=triggered_bot.id,
            bot_triggered_by=current_user.id,
            created_at=placeholder_at,
        )

        await broker.publish(
            room_id, {"type": "message", "message": bot_placeholder_message.model_dump()}
        )

        # Fire async task to generate real response
        asyncio.create_task(
            _generate_bot_response(
                room_id=room_id,
                placeholder_id=placeholder_id,
                bot=triggered_bot,
                triggering_message=body.content,
                triggering_user_id=current_user.id,
            )
        )

    return user_message


async def _generate_bot_response(
    room_id: str,
    placeholder_id: str,
    bot: BotConfig,
    triggering_message: str,
    triggering_user_id: str,
):
    try:
        db = get_db()

        # Fetch only the most recent messages to keep prompt size bounded.
        history_rows = db.execute(
            """
            SELECT h.content, h.is_bot
            FROM (
                SELECT m.content, m.is_bot, m.created_at
                FROM messages m
                WHERE m.room_id = ? AND m.id != ? AND m.content != 'thinking...'
                ORDER BY m.created_at DESC
                LIMIT ?
            ) h
            ORDER BY h.created_at ASC
            """,
            (room_id, placeholder_id, BOT_HISTORY_LIMIT),
        ).fetchall()

        history = [
            {"role": "assistant" if row["is_bot"] else "user", "content": row["content"]}
            for row in history_rows
        ]

        if BOT_DEBUG_CONTEXT:
            print(
                f"[bot-context] room={room_id} bot=@{bot.name} "
                f"history_count={len(history)} limit={BOT_HISTORY_LIMIT}",
                flush=True,
            )
            for idx, item in enumerate(history, start=1):
                snippet = item["content"].replace("\n", " ").strip()[:160]
                print(
                    f"[bot-context] {idx:02d} role={item['role']} content={snippet}",
                    flush=True,
                )
            trigger_snippet = triggering_message.replace("\n", " ").strip()[:160]
            print(f"[bot-context] trigger content={trigger_snippet}", flush=True)

        from ..models.bot import BotProvider
        from ..services.zenoh_bridge import zenoh_bridge

        if bot.provider == BotProvider.ZENOH:
            response_text = await zenoh_bridge.request(
                room_id=room_id,
                bot_name=bot.name,
                message=triggering_message,
                history=history,
                timeout=120.0,
            )
        else:
            loop = asyncio.get_event_loop()
            response_text = await loop.run_in_executor(
                None,
                lambda: build_bot_response(bot, triggering_message, history),
            )

        # Update placeholder message with actual response
        db.execute(
            "UPDATE messages SET content = ? WHERE id = ?",
            (response_text, placeholder_id),
        )
        db.commit()

        # Fetch updated message to emit
        updated_row = db.execute(
            """
            SELECT m.*, u.username
            FROM messages m
            JOIN users u ON m.user_id = u.id
            WHERE m.id = ?
            """,
            (placeholder_id,),
        ).fetchone()

        if updated_row:
            updated_msg = _row_to_message(updated_row)
            # Override username with bot name label
            updated_msg_dict = updated_msg.model_dump()
            updated_msg_dict["username"] = f"@{bot.name}"

            await broker.publish(
                room_id, {"type": "replace", "message": updated_msg_dict}
            )

    except Exception as exc:
        # Update placeholder with error text
        try:
            db = get_db()
            error_text = f"[Bot error: {str(exc)}]"
            db.execute(
                "UPDATE messages SET content = ? WHERE id = ?",
                (error_text, placeholder_id),
            )
            db.commit()

            updated_row = db.execute(
                """
                SELECT m.*, u.username
                FROM messages m
                JOIN users u ON m.user_id = u.id
                WHERE m.id = ?
                """,
                (placeholder_id,),
            ).fetchone()

            if not updated_row:
                return

            updated_msg = _row_to_message(updated_row)
            updated_msg_dict = updated_msg.model_dump()
            updated_msg_dict["username"] = f"@{bot.name}"

            await broker.publish(
                room_id,
                {
                    "type": "replace",
                    "message": updated_msg_dict,
                },
            )
        except Exception:
            pass


@router.get("/{room_id}/stream")
async def stream_room(
    room_id: str,
    request: Request,
    current_user: User = Depends(get_current_user),
):
    db = get_db()
    room = db.execute("SELECT id FROM rooms WHERE id = ?", (room_id,)).fetchone()
    if not room:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Room not found")

    async def event_generator():
        q = await broker.subscribe(room_id)
        try:
            while True:
                try:
                    data = await asyncio.wait_for(q.get(), timeout=15.0)
                    event_type = data.get("type", "message")
                    yield f"event: {event_type}\ndata: {json.dumps(data)}\n\n"
                except asyncio.TimeoutError:
                    yield "event: ping\ndata: \n\n"
        finally:
            broker.unsubscribe(room_id, q)

    return StreamingResponse(
        event_generator(),
        media_type="text/event-stream",
        headers={"Cache-Control": "no-cache", "X-Accel-Buffering": "no"},
    )
