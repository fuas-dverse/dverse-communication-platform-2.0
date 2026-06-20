"""Unit tests for backend/routes/messages.py.

Approach:
- In-memory SQLite provisioned via init_db().
- backend.db.get_db and backend.routes.messages.get_db are patched.
- broker.publish and zenoh_bridge are patched so no real I/O occurs.
- Async route functions are driven with asyncio.run().
"""

import asyncio
import sqlite3
import unittest
from unittest.mock import AsyncMock, MagicMock, patch

from fastapi import HTTPException

import backend.db as db_module
import backend.routes.messages as messages_routes
from backend.models.message import MessageCreate
from backend.models.user import User


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def make_test_db() -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:", check_same_thread=False)
    conn.execute("PRAGMA foreign_keys=ON")
    conn.row_factory = sqlite3.Row
    return conn


def _fake_user(uid: str = "user-1", username: str = "alice") -> User:
    return User(id=uid, username=username, created_at="2024-01-01T00:00:00Z")


def _seed_user(conn: sqlite3.Connection, uid: str = "user-1", username: str = "alice") -> None:
    conn.execute(
        "INSERT OR IGNORE INTO users (id, username, password_hash, created_at) VALUES (?, ?, ?, ?)",
        (uid, username, "hashed", "2024-01-01T00:00:00Z"),
    )
    conn.commit()


def _seed_room(
    conn: sqlite3.Connection,
    room_id: str = "room-1",
    name: str = "general",
    owner_id: str = "user-1",
) -> None:
    conn.execute(
        "INSERT OR IGNORE INTO rooms (id, name, description, created_by, created_at) VALUES (?, ?, ?, ?, ?)",
        (room_id, name, "", owner_id, "2024-01-01T00:00:00Z"),
    )
    conn.commit()


def _seed_bot(
    conn: sqlite3.Connection,
    bot_id: str = "bot-1",
    room_id: str = "room-1",
    name: str = "mybot",
    added_by: str = "user-1",
) -> None:
    conn.execute(
        "INSERT OR IGNORE INTO room_bots (id, room_id, name, provider, personality, created_at, added_by) VALUES (?, ?, ?, ?, ?, ?, ?)",
        (bot_id, room_id, name, "claude", "assistant", "2024-01-01T00:00:00Z", added_by),
    )
    conn.commit()


# ---------------------------------------------------------------------------
# Base test class
# ---------------------------------------------------------------------------

class MessagesTestCase(unittest.TestCase):
    def setUp(self) -> None:
        self.conn = make_test_db()

        db_patcher = patch.object(db_module, "get_db", return_value=self.conn)
        routes_patcher = patch("backend.routes.messages.get_db", return_value=self.conn)
        broker_patcher = patch("backend.routes.messages.broker.publish", new_callable=AsyncMock)
        # Patch zenoh_bridge to avoid import-time instantiation
        zenoh_patcher = patch("backend.services.zenoh_bridge.zenoh_bridge", new_callable=MagicMock)

        self.mock_db = db_patcher.start()
        self.mock_routes_db = routes_patcher.start()
        self.mock_publish = broker_patcher.start()
        self.mock_zenoh = zenoh_patcher.start()

        self.addCleanup(db_patcher.stop)
        self.addCleanup(routes_patcher.stop)
        self.addCleanup(broker_patcher.stop)
        self.addCleanup(zenoh_patcher.stop)

        db_module.init_db()
        _seed_user(self.conn)

    def tearDown(self) -> None:
        self.conn.close()

    def _run(self, coro):
        return asyncio.run(coro)


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

class TestGetMessages(MessagesTestCase):
    def test_get_messages_room_not_found(self) -> None:
        user = _fake_user()
        with self.assertRaises(HTTPException) as ctx:
            messages_routes.get_messages(room_id="ghost-room", after=None, current_user=user)
        self.assertEqual(ctx.exception.status_code, 404)

    def test_get_messages_empty(self) -> None:
        _seed_room(self.conn, room_id="room-1")
        user = _fake_user()
        result = messages_routes.get_messages(room_id="room-1", after=None, current_user=user)
        self.assertEqual(result, [])


class TestPostMessage(MessagesTestCase):
    def test_post_message_room_not_found(self) -> None:
        user = _fake_user()
        body = MessageCreate(content="hello")
        with self.assertRaises(HTTPException) as ctx:
            self._run(messages_routes.post_message(room_id="ghost-room", body=body, current_user=user))
        self.assertEqual(ctx.exception.status_code, 404)

    def test_post_message_success(self) -> None:
        _seed_room(self.conn, room_id="room-1")
        user = _fake_user()
        body = MessageCreate(content="hello world")
        result = self._run(messages_routes.post_message(room_id="room-1", body=body, current_user=user))
        self.assertEqual(result.content, "hello world")
        self.assertEqual(result.user_id, user.id)
        self.assertEqual(result.room_id, "room-1")
        self.assertFalse(result.is_bot)
        # Verify persisted
        row = self.conn.execute("SELECT content FROM messages WHERE id = ?", (result.id,)).fetchone()
        self.assertIsNotNone(row)
        self.assertEqual(row["content"], "hello world")

    def test_post_message_no_bot_trigger_when_no_at(self) -> None:
        """A message with no @mention must not schedule any asyncio task."""
        _seed_room(self.conn, room_id="room-1")
        _seed_bot(self.conn, bot_id="bot-1", room_id="room-1", name="mybot")
        user = _fake_user()
        body = MessageCreate(content="just a normal message, no mention")

        with patch("asyncio.create_task") as mock_create_task:
            self._run(messages_routes.post_message(room_id="room-1", body=body, current_user=user))
            mock_create_task.assert_not_called()


class TestPostMessageWithBotMention(MessagesTestCase):
    def test_post_message_with_bot_mention(self) -> None:
        _seed_room(self.conn, room_id="room-1")
        _seed_bot(self.conn, bot_id="bot-1", room_id="room-1", name="mybot")
        user = _fake_user()
        body = MessageCreate(content="@mybot hello there")

        with patch("asyncio.create_task") as mock_create_task:
            result = self._run(
                messages_routes.post_message(room_id="room-1", body=body, current_user=user)
            )
        self.assertEqual(result.content, "@mybot hello there")
        mock_create_task.assert_called_once()

    def test_post_message_with_unknown_mention(self) -> None:
        _seed_room(self.conn, room_id="room-1")
        user = _fake_user()
        body = MessageCreate(content="@nobody hello")

        with patch("asyncio.create_task") as mock_create_task:
            result = self._run(
                messages_routes.post_message(room_id="room-1", body=body, current_user=user)
            )
        mock_create_task.assert_not_called()
        self.assertEqual(result.content, "@nobody hello")


class TestFetchMessages(MessagesTestCase):
    def _insert_message(self, room_id: str, content: str, created_at: str, user_id: str = "user-1") -> None:
        import uuid
        self.conn.execute(
            "INSERT INTO messages (id, room_id, user_id, content, is_bot, created_at) VALUES (?, ?, ?, ?, 0, ?)",
            (str(uuid.uuid4()), room_id, user_id, content, created_at),
        )
        self.conn.commit()

    def test_fetch_messages_without_after(self) -> None:
        _seed_room(self.conn, room_id="room-1")
        self._insert_message("room-1", "msg1", "2024-01-01T10:00:00Z")
        self._insert_message("room-1", "msg2", "2024-01-01T11:00:00Z")
        results = messages_routes._fetch_messages(self.conn, "room-1")
        self.assertEqual(len(results), 2)

    def test_fetch_messages_with_after_filter(self) -> None:
        _seed_room(self.conn, room_id="room-1")
        self._insert_message("room-1", "early", "2024-01-01T09:00:00Z")
        self._insert_message("room-1", "late", "2024-01-01T11:00:00Z")
        results = messages_routes._fetch_messages(self.conn, "room-1", after="2024-01-01T10:00:00Z")
        self.assertEqual(len(results), 1)
        self.assertEqual(results[0].content, "late")


class TestGenerateBotResponse(MessagesTestCase):
    """Tests for the _generate_bot_response_inner async helper."""

    def _make_bot_config(self, bot_id="bot-1", room_id="room-1", name="mybot"):
        from backend.models.bot import BotConfig, BotProvider, BotPersonality
        return BotConfig(
            id=bot_id,
            room_id=room_id,
            name=name,
            provider=BotProvider.CLAUDE,
            personality=BotPersonality.ASSISTANT,
            created_at="2024-01-01T00:00:00Z",
        )

    def _insert_placeholder(self, room_id, placeholder_id, user_id, bot_id):
        self.conn.execute(
            """INSERT INTO messages (id, room_id, user_id, content, is_bot, bot_id, bot_triggered_by, created_at, bot_hop_count)
               VALUES (?, ?, ?, 'thinking...', 1, ?, ?, '2024-01-01T10:00:00Z', 0)""",
            (placeholder_id, room_id, user_id, bot_id, user_id),
        )
        self.conn.commit()

    def test_generate_bot_response_updates_placeholder(self) -> None:
        _seed_room(self.conn, room_id="room-1")
        _seed_bot(self.conn, bot_id="bot-1", room_id="room-1", name="mybot")
        bot = self._make_bot_config()
        placeholder_id = "placeholder-1"
        self._insert_placeholder("room-1", placeholder_id, "user-1", "bot-1")

        with patch("backend.routes.messages.build_bot_response", return_value="hello from bot"):
            self._run(
                messages_routes._generate_bot_response_inner(
                    room_id="room-1",
                    placeholder_id=placeholder_id,
                    bot=bot,
                    triggering_message="hello",
                    triggering_user_id="user-1",
                    bot_hop_count=0,
                )
            )

        row = self.conn.execute(
            "SELECT content FROM messages WHERE id = ?", (placeholder_id,)
        ).fetchone()
        self.assertEqual(row["content"], "hello from bot")

    def test_generate_bot_response_handles_exception(self) -> None:
        _seed_room(self.conn, room_id="room-1")
        _seed_bot(self.conn, bot_id="bot-1", room_id="room-1", name="mybot")
        bot = self._make_bot_config()
        placeholder_id = "placeholder-2"
        self._insert_placeholder("room-1", placeholder_id, "user-1", "bot-1")

        with patch("backend.routes.messages.build_bot_response", side_effect=RuntimeError("LLM down")):
            self._run(
                messages_routes._generate_bot_response_inner(
                    room_id="room-1",
                    placeholder_id=placeholder_id,
                    bot=bot,
                    triggering_message="hello",
                    triggering_user_id="user-1",
                    bot_hop_count=0,
                )
            )

        row = self.conn.execute(
            "SELECT content FROM messages WHERE id = ?", (placeholder_id,)
        ).fetchone()
        self.assertIn("Bot error", row["content"])

    def test_generate_bot_response_chain_mention(self) -> None:
        """Bot reply with @otherbot mention should create a chained placeholder."""
        _seed_room(self.conn, room_id="room-1")
        _seed_bot(self.conn, bot_id="bot-1", room_id="room-1", name="mybot")
        _seed_bot(self.conn, bot_id="bot-2", room_id="room-1", name="otherbot")
        bot = self._make_bot_config()
        placeholder_id = "placeholder-3"
        self._insert_placeholder("room-1", placeholder_id, "user-1", "bot-1")

        with patch("backend.routes.messages.build_bot_response", return_value="@otherbot check this"):
            with patch("asyncio.create_task") as mock_create_task:
                self._run(
                    messages_routes._generate_bot_response_inner(
                        room_id="room-1",
                        placeholder_id=placeholder_id,
                        bot=bot,
                        triggering_message="hello",
                        triggering_user_id="user-1",
                        bot_hop_count=0,
                    )
                )
        mock_create_task.assert_called_once()


class TestRowToMessage(unittest.TestCase):
    """Tests for the _row_to_message helper."""

    def _make_row(self, **overrides):
        """Build a minimal dict that mimics a sqlite3.Row for _row_to_message."""
        defaults = {
            "id": "msg-1",
            "room_id": "room-1",
            "user_id": "user-1",
            "username": "alice",
            "content": "hi",
            "is_bot": 0,
            "bot_id": None,
            "bot_name": None,
            "bot_triggered_by": None,
            "bot_hop_count": 0,
            "created_at": "2024-01-01T00:00:00Z",
        }
        defaults.update(overrides)
        return defaults

    def test_row_to_message_is_bot_true(self) -> None:
        row = self._make_row(is_bot=1, bot_name="mybot", user_id="user-1")
        msg = messages_routes._row_to_message(row)
        self.assertTrue(msg.is_bot)
        self.assertEqual(msg.username, "@mybot")

    def test_row_to_message_is_bot_false(self) -> None:
        row = self._make_row(is_bot=0, username="carol", bot_name=None)
        msg = messages_routes._row_to_message(row)
        self.assertFalse(msg.is_bot)
        self.assertEqual(msg.username, "carol")

    def test_row_to_message_bot_no_name(self) -> None:
        """is_bot=True but bot_name=None falls back to username."""
        row = self._make_row(is_bot=1, bot_name=None, username="alice")
        msg = messages_routes._row_to_message(row)
        self.assertTrue(msg.is_bot)
        self.assertEqual(msg.username, "alice")
