"""Tests for backend/services/a2a.py.

Strategy:
- In-memory SQLite provisioned via init_db().
- zenoh_bridge.request, broker.publish, and telemetry are all mocked.
- run_a2a_session is driven directly with asyncio.run().
"""

import asyncio
import sqlite3
import unittest
from unittest.mock import AsyncMock, MagicMock, patch

import backend.db as db_module
from backend.models.bot import BotConfig, BotPersonality, BotProvider


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def make_test_db() -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:", check_same_thread=False)
    conn.execute("PRAGMA foreign_keys=ON")
    conn.row_factory = sqlite3.Row
    return conn


def _seed_user(conn, uid="user-1", username="alice"):
    conn.execute(
        "INSERT OR IGNORE INTO users (id, username, password_hash, created_at) VALUES (?, ?, ?, ?)",
        (uid, username, "hashed", "2024-01-01T00:00:00Z"),
    )
    conn.commit()


def _seed_room(conn, room_id="room-1"):
    conn.execute(
        "INSERT OR IGNORE INTO rooms (id, name, description, created_by, created_at) VALUES (?, ?, ?, ?, ?)",
        (room_id, "general", "", "user-1", "2024-01-01T00:00:00Z"),
    )
    conn.commit()


def _seed_bot(conn, bot_id, room_id, name, provider="zenoh"):
    conn.execute(
        "INSERT OR IGNORE INTO room_bots (id, room_id, name, provider, personality, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        (bot_id, room_id, name, provider, "assistant", "2024-01-01T00:00:00Z"),
    )
    conn.commit()


def _make_bot(bot_id, room_id, name, provider=BotProvider.ZENOH):
    return BotConfig(
        id=bot_id,
        room_id=room_id,
        name=name,
        provider=provider,
        personality=BotPersonality.ASSISTANT,
        created_at="2024-01-01T00:00:00Z",
    )


# ---------------------------------------------------------------------------
# Base test class
# ---------------------------------------------------------------------------

class A2ATestCase(unittest.TestCase):
    def setUp(self):
        self.conn = make_test_db()

        db_patcher = patch.object(db_module, "get_db", return_value=self.conn)
        a2a_db_patcher = patch("backend.services.a2a.get_db", return_value=self.conn)
        broker_patcher = patch("backend.services.a2a.broker.publish", new_callable=AsyncMock)
        # Stub out telemetry counters so they don't fail without an OTel provider
        self.mock_a2a_sessions = MagicMock()
        self.mock_a2a_turns = MagicMock()
        self.mock_a2a_session_duration = MagicMock()
        self.mock_a2a_turn_duration = MagicMock()
        tel_patcher = patch.multiple(
            "backend.services.a2a",
            a2a_sessions=self.mock_a2a_sessions,
            a2a_turns=self.mock_a2a_turns,
            a2a_session_duration=self.mock_a2a_session_duration,
            a2a_turn_duration=self.mock_a2a_turn_duration,
        )

        self.mock_db = db_patcher.start()
        self.mock_a2a_db = a2a_db_patcher.start()
        self.mock_publish = broker_patcher.start()
        tel_patcher.start()

        self.addCleanup(db_patcher.stop)
        self.addCleanup(a2a_db_patcher.stop)
        self.addCleanup(broker_patcher.stop)
        self.addCleanup(tel_patcher.stop)

        db_module.init_db()
        _seed_user(self.conn)

    def tearDown(self):
        self.conn.close()

    def _run(self, coro):
        return asyncio.run(coro)


# ---------------------------------------------------------------------------
# _post_a2a_message
# ---------------------------------------------------------------------------

class TestPostA2AMessage(A2ATestCase):
    def test_inserts_message_and_returns_model(self):
        from backend.services.a2a import _post_a2a_message
        _seed_room(self.conn)
        _seed_bot(self.conn, "bot-1", "room-1", "alpha")
        bot = _make_bot("bot-1", "room-1", "alpha")

        msg = _post_a2a_message(self.conn, "room-1", "user-1", bot, "hello world", hop_count=1)

        self.assertEqual(msg.content, "hello world")
        self.assertTrue(msg.is_bot)
        self.assertEqual(msg.username, "@alpha")
        self.assertEqual(msg.bot_triggered_by, "a2a_council")
        self.assertEqual(msg.bot_hop_count, 1)

        row = self.conn.execute(
            "SELECT content, bot_triggered_by, bot_hop_count FROM messages WHERE id = ?",
            (msg.id,),
        ).fetchone()
        self.assertIsNotNone(row)
        self.assertEqual(row["content"], "hello world")
        self.assertEqual(row["bot_triggered_by"], "a2a_council")
        self.assertEqual(row["bot_hop_count"], 1)


# ---------------------------------------------------------------------------
# _call_bot
# ---------------------------------------------------------------------------

class TestCallBot(A2ATestCase):
    def test_calls_zenoh_bridge(self):
        from backend.services.a2a import _call_bot
        bot = _make_bot("bot-1", "room-1", "alpha", provider=BotProvider.ZENOH)

        mock_bridge = MagicMock()
        mock_bridge.request = AsyncMock(return_value="zenoh reply")

        with patch("backend.services.zenoh_bridge.zenoh_bridge", mock_bridge):
            result = self._run(_call_bot(bot, "hello", [{"role": "user", "content": "hi"}], "room-1"))

        self.assertEqual(result, "zenoh reply")
        mock_bridge.request.assert_called_once_with(
            room_id="room-1",
            bot_name="alpha",
            message="hello",
            history=[{"role": "user", "content": "hi"}],
            timeout=120.0,
        )


# ---------------------------------------------------------------------------
# run_a2a_session — happy path
# ---------------------------------------------------------------------------

class TestRunA2ASession(A2ATestCase):
    def _setup_room_and_bots(self):
        _seed_room(self.conn)
        _seed_bot(self.conn, "bot-1", "room-1", "alpha")
        _seed_bot(self.conn, "bot-2", "room-1", "beta")
        bot1 = _make_bot("bot-1", "room-1", "alpha")
        bot2 = _make_bot("bot-2", "room-1", "beta")
        return bot1, bot2

    def test_single_turn_inserts_messages(self):
        from backend.services.a2a import run_a2a_session
        bot1, bot2 = self._setup_room_and_bots()

        reply_seq = ["alpha reply", "beta reply", "alpha synthesis"]
        call_count = {"n": 0}

        async def fake_request(**kwargs):
            r = reply_seq[call_count["n"]]
            call_count["n"] += 1
            return r

        mock_bridge = MagicMock()
        mock_bridge.request = fake_request

        with patch("backend.services.zenoh_bridge.zenoh_bridge", mock_bridge):
            self._run(run_a2a_session(
                room_id="room-1",
                bot1=bot1,
                bot2=bot2,
                prompt="debate topic",
                turns=1,
                triggering_user_id="user-1",
            ))

        rows = self.conn.execute(
            "SELECT content FROM messages ORDER BY created_at ASC"
        ).fetchall()
        contents = [r["content"] for r in rows]

        self.assertTrue(any("Starting" in c for c in contents))
        self.assertIn("alpha reply", contents)
        self.assertIn("beta reply", contents)
        self.assertIn("alpha synthesis", contents)

    def test_telemetry_recorded(self):
        from backend.services.a2a import run_a2a_session
        bot1, bot2 = self._setup_room_and_bots()

        mock_bridge = MagicMock()
        mock_bridge.request = AsyncMock(return_value="reply")

        with patch("backend.services.zenoh_bridge.zenoh_bridge", mock_bridge):
            self._run(run_a2a_session(
                room_id="room-1",
                bot1=bot1,
                bot2=bot2,
                prompt="topic",
                turns=1,
                triggering_user_id="user-1",
            ))

        self.mock_a2a_sessions.add.assert_called_once()
        self.mock_a2a_turns.record.assert_called_once()
        self.mock_a2a_session_duration.record.assert_called_once()
        self.assertTrue(self.mock_a2a_turn_duration.record.called)

    def test_sse_events_published(self):
        from backend.services.a2a import run_a2a_session
        bot1, bot2 = self._setup_room_and_bots()

        mock_bridge = MagicMock()
        mock_bridge.request = AsyncMock(return_value="reply")

        with patch("backend.services.zenoh_bridge.zenoh_bridge", mock_bridge):
            self._run(run_a2a_session(
                room_id="room-1",
                bot1=bot1,
                bot2=bot2,
                prompt="topic",
                turns=1,
                triggering_user_id="user-1",
            ))

        # start + bot1 reply + bot2 reply + synthesis = 4 publishes minimum
        self.assertGreaterEqual(self.mock_publish.call_count, 4)

    def test_multiple_turns(self):
        from backend.services.a2a import run_a2a_session
        bot1, bot2 = self._setup_room_and_bots()

        mock_bridge = MagicMock()
        mock_bridge.request = AsyncMock(return_value="some reply")

        with patch("backend.services.zenoh_bridge.zenoh_bridge", mock_bridge):
            self._run(run_a2a_session(
                room_id="room-1",
                bot1=bot1,
                bot2=bot2,
                prompt="topic",
                turns=3,
                triggering_user_id="user-1",
            ))

        # 3 turns × 2 bots + synthesis = 7 bot calls; start message = 1
        # zenoh_bridge.request called for each bot turn + synthesis
        self.assertEqual(mock_bridge.request.call_count, 7)  # 3*2 turns + 1 synthesis


# ---------------------------------------------------------------------------
# run_a2a_session — error handling
# ---------------------------------------------------------------------------

class TestRunA2ASessionErrors(A2ATestCase):
    def _setup(self):
        _seed_room(self.conn)
        _seed_bot(self.conn, "bot-1", "room-1", "alpha")
        _seed_bot(self.conn, "bot-2", "room-1", "beta")
        return _make_bot("bot-1", "room-1", "alpha"), _make_bot("bot-2", "room-1", "beta")

    def test_bot_error_posts_error_message(self):
        from backend.services.a2a import run_a2a_session
        bot1, bot2 = self._setup()

        mock_bridge = MagicMock()
        mock_bridge.request = AsyncMock(side_effect=RuntimeError("bot is down"))

        with patch("backend.services.zenoh_bridge.zenoh_bridge", mock_bridge):
            self._run(run_a2a_session(
                room_id="room-1",
                bot1=bot1,
                bot2=bot2,
                prompt="topic",
                turns=2,
                triggering_user_id="user-1",
            ))

        rows = self.conn.execute(
            "SELECT content FROM messages WHERE content LIKE '%A2A Error%'"
        ).fetchall()
        self.assertTrue(len(rows) >= 1)
        self.assertIn("bot is down", rows[0]["content"])

    def test_zero_turns_completed_skips_synthesis(self):
        from backend.services.a2a import run_a2a_session
        bot1, bot2 = self._setup()

        mock_bridge = MagicMock()
        mock_bridge.request = AsyncMock(side_effect=RuntimeError("fail"))

        with patch("backend.services.zenoh_bridge.zenoh_bridge", mock_bridge):
            self._run(run_a2a_session(
                room_id="room-1",
                bot1=bot1,
                bot2=bot2,
                prompt="topic",
                turns=1,
                triggering_user_id="user-1",
            ))

        # Synthesis should NOT run — check no "Synthesis" content posted
        rows = self.conn.execute(
            "SELECT content FROM messages WHERE content LIKE '%Synthesis%'"
        ).fetchall()
        self.assertEqual(len(rows), 0)

    def test_synthesis_error_posts_error_message(self):
        """When turns succeed but synthesis fails, an error message is posted."""
        from backend.services.a2a import run_a2a_session
        bot1, bot2 = self._setup()

        call_count = {"n": 0}

        async def fail_on_synthesis(**kwargs):
            call_count["n"] += 1
            if call_count["n"] >= 3:  # first 2 are turn calls, 3rd is synthesis
                raise RuntimeError("synthesis failed")
            return "reply"

        mock_bridge = MagicMock()
        mock_bridge.request = fail_on_synthesis

        with patch("backend.services.zenoh_bridge.zenoh_bridge", mock_bridge):
            self._run(run_a2a_session(
                room_id="room-1",
                bot1=bot1,
                bot2=bot2,
                prompt="topic",
                turns=1,
                triggering_user_id="user-1",
            ))

        rows = self.conn.execute(
            "SELECT content FROM messages WHERE content LIKE '%Synthesis Error%'"
        ).fetchall()
        self.assertEqual(len(rows), 1)
        self.assertIn("synthesis failed", rows[0]["content"])
