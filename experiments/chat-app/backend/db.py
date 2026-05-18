import sqlite3
import unittest
from unittest.mock import patch

import backend.db as db_module

import threading
from pathlib import Path

DB_PATH = Path(__file__).parent.parent / "chatapp.db"

_local = threading.local()

def make_test_db() -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:", check_same_thread=False)
    conn.execute("PRAGMA foreign_keys=ON")
    conn.row_factory = sqlite3.Row
    return conn


class TestAuth(unittest.TestCase):
    def setUp(self) -> None:
        self.conn = make_test_db()
        patcher = patch.object(db_module, "get_db", return_value=self.conn)
        self.mock_get_db = patcher.start()
        self.addCleanup(patcher.stop)
        db_module.init_db()

    def tearDown(self) -> None:
        self.conn.close()

def get_db() -> sqlite3.Connection:
    if not hasattr(_local, "conn") or _local.conn is None:
        conn = sqlite3.connect(str(DB_PATH), check_same_thread=False)
        conn.execute("PRAGMA journal_mode=WAL")
        conn.execute("PRAGMA foreign_keys=ON")
        conn.row_factory = sqlite3.Row
        _local.conn = conn
    return _local.conn


def init_db():
    conn = get_db()
    conn.executescript(
        """
        CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            username TEXT UNIQUE NOT NULL,
            password_hash TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS servers (
            id TEXT PRIMARY KEY,
            name TEXT UNIQUE NOT NULL,
            description TEXT DEFAULT '',
            created_by TEXT REFERENCES users(id),
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS server_members (
            server_id TEXT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            username TEXT NOT NULL,
            joined_at TEXT NOT NULL,
            PRIMARY KEY (server_id, user_id)
        );

        CREATE TABLE IF NOT EXISTS rooms (
            id TEXT PRIMARY KEY,
            name TEXT UNIQUE NOT NULL,
            description TEXT DEFAULT '',
            created_by TEXT REFERENCES users(id),
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS room_bots (
            id TEXT PRIMARY KEY,
            room_id TEXT NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            provider TEXT NOT NULL,
            personality TEXT NOT NULL DEFAULT 'assistant',
            model TEXT,
            created_at TEXT NOT NULL,
            UNIQUE(room_id, name)
        );

        CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            room_id TEXT NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
            user_id TEXT NOT NULL REFERENCES users(id),
            content TEXT NOT NULL,
            is_bot INTEGER DEFAULT 0,
            bot_id TEXT REFERENCES room_bots(id),
            bot_triggered_by TEXT,
            created_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_messages_room ON messages(room_id, created_at);
        CREATE INDEX IF NOT EXISTS idx_server_members_server ON server_members(server_id);
        CREATE INDEX IF NOT EXISTS idx_server_members_user ON server_members(user_id);
        """
    )
    conn.commit()

    # Migrations — add columns to existing tables when upgrading
    _migrate(conn)


def _migrate(conn: sqlite3.Connection):
    """Apply schema migrations idempotently."""
    # Add server_id to rooms if not present
    existing_cols = {row[1] for row in conn.execute("PRAGMA table_info(rooms)").fetchall()}
    if "server_id" not in existing_cols:
        conn.execute(
            "ALTER TABLE rooms ADD COLUMN server_id TEXT REFERENCES servers(id)"
        )
        conn.commit()

    # Add invite_code to servers if not present
    server_cols = {row[1] for row in conn.execute("PRAGMA table_info(servers)").fetchall()}
    if "invite_code" not in server_cols:
        conn.execute("ALTER TABLE servers ADD COLUMN invite_code TEXT")
        conn.execute("CREATE UNIQUE INDEX IF NOT EXISTS idx_servers_invite_code ON servers(invite_code) WHERE invite_code IS NOT NULL")
        conn.commit()

    # Add is_online and last_seen to server_members if not present
    member_cols = {row[1] for row in conn.execute("PRAGMA table_info(server_members)").fetchall()}
    if "is_online" not in member_cols:
        conn.execute("ALTER TABLE server_members ADD COLUMN is_online INTEGER DEFAULT 0")
        conn.commit()
    if "last_seen" not in member_cols:
        conn.execute("ALTER TABLE server_members ADD COLUMN last_seen TEXT")
        conn.commit()
