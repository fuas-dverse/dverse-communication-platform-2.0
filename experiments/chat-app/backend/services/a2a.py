import time
import uuid
from datetime import datetime

from ..db import get_db
from ..models.bot import BotConfig
from ..models.message import Message
from ..services.sse import broker
from ..telemetry import a2a_sessions, a2a_session_duration, a2a_turns, a2a_turn_duration


async def _call_bot(bot: BotConfig, message: str, history: list[dict], room_id: str) -> str:
    from .zenoh_bridge import zenoh_bridge
    return await zenoh_bridge.request(
        room_id=room_id,
        bot_name=bot.name,
        message=message,
        history=history,
        timeout=120.0,
    )


def _post_a2a_message(
    db,
    room_id: str,
    user_id: str,
    bot: BotConfig,
    content: str,
    hop_count: int,
) -> Message:
    msg_id = str(uuid.uuid4())
    created_at = datetime.utcnow().isoformat() + "Z"
    db.execute(
        """INSERT INTO messages
           (id, room_id, user_id, content, is_bot, bot_id, bot_triggered_by, created_at, bot_hop_count)
           VALUES (?, ?, ?, ?, 1, ?, 'a2a_council', ?, ?)""",
        (msg_id, room_id, user_id, content, bot.id, created_at, hop_count),
    )
    db.commit()
    return Message(
        id=msg_id,
        room_id=room_id,
        user_id=user_id,
        username=f"@{bot.name}",
        content=content,
        is_bot=True,
        bot_id=bot.id,
        bot_triggered_by="a2a_council",
        bot_hop_count=hop_count,
        created_at=created_at,
    )


async def run_a2a_session(
    room_id: str,
    bot1: BotConfig,
    bot2: BotConfig,
    prompt: str,
    turns: int,
    triggering_user_id: str,
) -> None:
    db = get_db()
    session_attrs = {"a2a.bot1": bot1.name, "a2a.bot2": bot2.name}
    a2a_sessions.add(1, session_attrs)
    t_start = time.monotonic()

    start_msg = _post_a2a_message(
        db, room_id, triggering_user_id, bot1,
        f"[A2A Council] Starting {turns}-turn dialogue: {prompt}",
        hop_count=0,
    )
    await broker.publish(room_id, {"type": "message", "message": start_msg.model_dump()})

    history: list[dict] = [{"role": "user", "content": prompt}]
    turns_completed = 0

    for turn in range(1, turns + 1):
        try:
            t = time.monotonic()
            frame1 = (
                f"[A2A Turn {turn}/{turns}] You are in a structured AI council debating with @{bot2.name}. "
                f"Respond to: {history[-1]['content']}"
            )
            reply1 = await _call_bot(bot1, frame1, history, room_id)
            a2a_turn_duration.record(
                time.monotonic() - t, {**session_attrs, "a2a.speaker": bot1.name}
            )

            msg1 = _post_a2a_message(
                db, room_id, triggering_user_id, bot1, reply1, hop_count=turn
            )
            await broker.publish(room_id, {"type": "message", "message": msg1.model_dump()})
            history.append({"role": "assistant", "content": reply1})

            t = time.monotonic()
            frame2 = (
                f"[A2A Turn {turn}/{turns}] You are in a structured AI council. "
                f"@{bot1.name} said: {reply1}\n\nRespond directly to their perspective."
            )
            reply2 = await _call_bot(bot2, frame2, history, room_id)
            a2a_turn_duration.record(
                time.monotonic() - t, {**session_attrs, "a2a.speaker": bot2.name}
            )

            msg2 = _post_a2a_message(
                db, room_id, triggering_user_id, bot2, reply2, hop_count=turn
            )
            await broker.publish(room_id, {"type": "message", "message": msg2.model_dump()})
            history.append({"role": "user", "content": reply2})

            turns_completed += 1

        except Exception as exc:
            err = _post_a2a_message(
                db, room_id, triggering_user_id, bot1,
                f"[A2A Error at turn {turn}]: {exc}",
                hop_count=turn,
            )
            await broker.publish(room_id, {"type": "message", "message": err.model_dump()})
            break

    if turns_completed > 0:
        try:
            t = time.monotonic()
            synth_reply = await _call_bot(
                bot1,
                "[A2A Synthesis] The council has concluded. Summarize the key conclusions, "
                "points of agreement, and unresolved tensions from this dialogue.",
                history,
                room_id,
            )
            a2a_turn_duration.record(
                time.monotonic() - t,
                {**session_attrs, "a2a.speaker": f"{bot1.name}:synthesis"},
            )
            synth_msg = _post_a2a_message(
                db, room_id, triggering_user_id, bot1,
                synth_reply, hop_count=turns_completed + 1,
            )
            await broker.publish(room_id, {"type": "message", "message": synth_msg.model_dump()})
        except Exception as exc:
            err = _post_a2a_message(
                db, room_id, triggering_user_id, bot1,
                f"[A2A Synthesis Error]: {exc}",
                hop_count=turns_completed + 1,
            )
            await broker.publish(room_id, {"type": "message", "message": err.model_dump()})

    a2a_turns.record(turns_completed, session_attrs)
    a2a_session_duration.record(time.monotonic() - t_start, session_attrs)
