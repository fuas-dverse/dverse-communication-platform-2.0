import Database from 'better-sqlite3'
import path from 'path'

const DB_PATH = path.join(process.cwd(), 'chatapp.db')

declare global {
  // eslint-disable-next-line no-var
  var __db: Database.Database | undefined
}

const SCHEMA_SQL = `
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS users (
  id           TEXT PRIMARY KEY,
  username     TEXT UNIQUE NOT NULL,
  password_hash TEXT NOT NULL,
  created_at   INTEGER NOT NULL
);

-- Seeded system user for bot messages
INSERT OR IGNORE INTO users (id, username, password_hash, created_at)
VALUES ('bot-system-user', '_system_bot', 'n/a', 0);

CREATE TABLE IF NOT EXISTS rooms (
  id           TEXT PRIMARY KEY,
  name         TEXT UNIQUE NOT NULL,
  description  TEXT,
  created_by   TEXT NOT NULL REFERENCES users(id),
  has_bot      INTEGER NOT NULL DEFAULT 0,
  bot_name     TEXT NOT NULL DEFAULT 'bot',
  bot_provider TEXT NOT NULL DEFAULT 'claude',
  created_at   INTEGER NOT NULL
);


CREATE TABLE IF NOT EXISTS messages (
  id               TEXT PRIMARY KEY,
  room_id          TEXT NOT NULL REFERENCES rooms(id),
  user_id          TEXT NOT NULL REFERENCES users(id),
  content          TEXT NOT NULL,
  is_bot           INTEGER NOT NULL DEFAULT 0,
  bot_triggered_by TEXT,
  created_at       INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_messages_room_created
  ON messages(room_id, created_at);
`

function createDb(): Database.Database {
  const db = new Database(DB_PATH, { timeout: 5000 })
  db.pragma('journal_mode = WAL')
  db.pragma('foreign_keys = ON')
  db.exec(SCHEMA_SQL)
  // Migrate existing DBs — SQLite has no ADD COLUMN IF NOT EXISTS
  try { db.exec(`ALTER TABLE rooms ADD COLUMN bot_provider TEXT NOT NULL DEFAULT 'claude'`) } catch { /* already exists */ }
  return db
}

export const db: Database.Database =
  globalThis.__db ?? (globalThis.__db = createDb())

if (process.env.NODE_ENV === 'development') {
  globalThis.__db = db
}

// ── Prepared statements ───────────────────────────────────────────────────────

export const stmts = {
  // users
  getUserByUsername: db.prepare(
    'SELECT * FROM users WHERE username = ?'
  ),
  getUserById: db.prepare(
    'SELECT id, username, created_at FROM users WHERE id = ?'
  ),
  insertUser: db.prepare(
    'INSERT INTO users (id, username, password_hash, created_at) VALUES (?, ?, ?, ?)'
  ),

  // rooms
  getRooms: db.prepare(
    'SELECT * FROM rooms ORDER BY created_at DESC'
  ),
  getRoomById: db.prepare(
    'SELECT * FROM rooms WHERE id = ?'
  ),
  insertRoom: db.prepare(
    'INSERT INTO rooms (id, name, description, created_by, has_bot, bot_name, bot_provider, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)'
  ),
  updateRoomBot: db.prepare(
    'UPDATE rooms SET has_bot = ?, bot_name = ?, bot_provider = ? WHERE id = ?'
  ),

  // messages
  getMessages: db.prepare(`
    SELECT m.*, u.username
    FROM messages m
    JOIN users u ON m.user_id = u.id
    WHERE m.room_id = ?
    ORDER BY m.created_at ASC
    LIMIT 100
  `),
  getMessagesAfter: db.prepare(`
    SELECT m.*, u.username
    FROM messages m
    JOIN users u ON m.user_id = u.id
    WHERE m.room_id = ? AND m.created_at > ?
    ORDER BY m.created_at ASC
  `),
  getLastNMessages: db.prepare(`
    SELECT m.*, u.username
    FROM messages m
    JOIN users u ON m.user_id = u.id
    WHERE m.room_id = ?
    ORDER BY m.created_at DESC
    LIMIT ?
  `),
  insertMessage: db.prepare(
    'INSERT INTO messages (id, room_id, user_id, content, is_bot, bot_triggered_by, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)'
  ),
  getMessageById: db.prepare(`
    SELECT m.*, u.username
    FROM messages m
    JOIN users u ON m.user_id = u.id
    WHERE m.id = ?
  `),
  updateMessageContent: db.prepare(
    'UPDATE messages SET content = ? WHERE id = ?'
  ),
}

export const BOT_USER_ID = 'bot-system-user'
