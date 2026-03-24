import sqlite3
import threading
from pathlib import Path

DB_PATH = Path(__file__).parent.parent / "chatapp.db"

_local = threading.local()


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
        """
    )
    conn.commit()
