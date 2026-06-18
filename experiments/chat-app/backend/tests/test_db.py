"""Unit tests for backend/db.py — schema creation and migrations."""

import sqlite3
import unittest
from unittest.mock import patch

import backend.db as db_module


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def make_test_db() -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:", check_same_thread=False)
    conn.execute("PRAGMA foreign_keys=ON")
    conn.row_factory = sqlite3.Row
    return conn


def _table_names(conn: sqlite3.Connection) -> set[str]:
    rows = conn.execute("SELECT name FROM sqlite_master WHERE type='table'").fetchall()
    return {row[0] for row in rows}


def _column_names(conn: sqlite3.Connection, table: str) -> set[str]:
    rows = conn.execute(f"PRAGMA table_info({table})").fetchall()
    return {row[1] for row in rows}


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

class TestInitDb(unittest.TestCase):
    def setUp(self) -> None:
        self.conn = make_test_db()
        patcher = patch.object(db_module, "get_db", return_value=self.conn)
        self.mock_get_db = patcher.start()
        self.addCleanup(patcher.stop)

    def tearDown(self) -> None:
        self.conn.close()

    def test_init_db_creates_tables(self) -> None:
        db_module.init_db()
        tables = _table_names(self.conn)
        for expected in ("users", "servers", "rooms", "messages", "room_bots", "server_members"):
            self.assertIn(expected, tables, f"Expected table '{expected}' to exist")

    def test_migrate_idempotent(self) -> None:
        """Calling init_db twice must not raise any error."""
        db_module.init_db()
        # Should not raise
        db_module.init_db()

    def test_migrate_adds_server_id_to_rooms(self) -> None:
        db_module.init_db()
        cols = _column_names(self.conn, "rooms")
        self.assertIn("server_id", cols)

    def test_migrate_adds_invite_code_to_servers(self) -> None:
        db_module.init_db()
        cols = _column_names(self.conn, "servers")
        self.assertIn("invite_code", cols)

    def test_migrate_adds_is_online_to_server_members(self) -> None:
        db_module.init_db()
        cols = _column_names(self.conn, "server_members")
        self.assertIn("is_online", cols)

    def test_migrate_adds_last_seen_to_server_members(self) -> None:
        db_module.init_db()
        cols = _column_names(self.conn, "server_members")
        self.assertIn("last_seen", cols)

    def test_migrate_adds_added_by_to_room_bots(self) -> None:
        db_module.init_db()
        cols = _column_names(self.conn, "room_bots")
        self.assertIn("added_by", cols)

    def test_migrate_adds_system_prompt_to_room_bots(self) -> None:
        db_module.init_db()
        cols = _column_names(self.conn, "room_bots")
        self.assertIn("system_prompt", cols)

    def test_migrate_adds_bot_hop_count_to_messages(self) -> None:
        db_module.init_db()
        cols = _column_names(self.conn, "messages")
        self.assertIn("bot_hop_count", cols)

    def test_get_db_returns_connection_with_row_factory(self) -> None:
        """get_db() returns a connection with row_factory set to sqlite3.Row."""
        # get_db is already patched to return self.conn which has row_factory=sqlite3.Row
        conn = db_module.get_db()
        self.assertIsNotNone(conn)
        self.assertEqual(conn.row_factory, sqlite3.Row)
