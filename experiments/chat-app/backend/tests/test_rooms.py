"""Unit tests for backend/routes/rooms.py.

Approach:
- In-memory SQLite provisioned via init_db().
- backend.db.get_db and backend.routes.rooms.get_db are both patched to return
  the same in-memory connection.
- get_current_user is patched at the route-module level so no JWT is needed.
- broker.publish is patched as AsyncMock so SSE side-effects are silenced.
- Async route functions are driven with asyncio.run().
"""

import asyncio
import sqlite3
import unittest
from unittest.mock import AsyncMock, patch

from fastapi import HTTPException

import backend.db as db_module
import backend.routes.rooms as rooms_routes
from backend.models.bot import BotConfigCreate, BotConfigUpdate, BotProvider, BotPersonality
from backend.models.room import RoomCreate
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


def _seed_server(conn: sqlite3.Connection, server_id: str = "srv-1", owner_id: str = "user-1") -> None:
    conn.execute(
        "INSERT OR IGNORE INTO servers (id, name, description, created_by, created_at) VALUES (?, ?, ?, ?, ?)",
        (server_id, "Test Server", "", owner_id, "2024-01-01T00:00:00Z"),
    )
    conn.commit()


def _seed_room(
    conn: sqlite3.Connection,
    room_id: str = "room-1",
    name: str = "general",
    owner_id: str = "user-1",
    server_id: str | None = None,
) -> None:
    conn.execute(
        "INSERT OR IGNORE INTO rooms (id, name, description, created_by, created_at, server_id) VALUES (?, ?, ?, ?, ?, ?)",
        (room_id, name, "", owner_id, "2024-01-01T00:00:00Z", server_id),
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

class RoomsTestCase(unittest.TestCase):
    def setUp(self) -> None:
        self.conn = make_test_db()

        db_patcher = patch.object(db_module, "get_db", return_value=self.conn)
        routes_patcher = patch("backend.routes.rooms.get_db", return_value=self.conn)
        broker_patcher = patch("backend.routes.rooms.broker.publish", new_callable=AsyncMock)

        self.mock_db = db_patcher.start()
        self.mock_routes_db = routes_patcher.start()
        self.mock_publish = broker_patcher.start()

        self.addCleanup(db_patcher.stop)
        self.addCleanup(routes_patcher.stop)
        self.addCleanup(broker_patcher.stop)

        db_module.init_db()

        # Seed a default user
        _seed_user(self.conn)

    def tearDown(self) -> None:
        self.conn.close()

    def _run(self, coro):
        return asyncio.run(coro)


# ---------------------------------------------------------------------------
# Tests: list / create / get / delete rooms
# ---------------------------------------------------------------------------

class TestListRoomsEmpty(RoomsTestCase):
    def test_list_rooms_empty(self) -> None:
        user = _fake_user()
        result = rooms_routes.list_rooms(current_user=user)
        self.assertEqual(result, [])


class TestCreateRoom(RoomsTestCase):
    def test_create_room_success(self) -> None:
        user = _fake_user()
        body = RoomCreate(name="lobby", description="main chat")
        result = self._run(rooms_routes.create_room(body=body, current_user=user))
        self.assertEqual(result.name, "lobby")
        self.assertEqual(result.description, "main chat")
        self.assertEqual(result.created_by, user.id)
        self.assertIsNotNone(result.id)

    def test_create_room_duplicate_name(self) -> None:
        user = _fake_user()
        body = RoomCreate(name="lobby")
        self._run(rooms_routes.create_room(body=body, current_user=user))
        with self.assertRaises(HTTPException) as ctx:
            self._run(rooms_routes.create_room(body=body, current_user=user))
        self.assertEqual(ctx.exception.status_code, 409)

    def test_create_room_server_membership_required(self) -> None:
        _seed_server(self.conn, server_id="srv-1", owner_id="user-1")
        # user-2 is NOT a member of srv-1
        _seed_user(self.conn, uid="user-2", username="bob")
        user2 = _fake_user(uid="user-2", username="bob")
        body = RoomCreate(name="private-room", server_id="srv-1")
        with self.assertRaises(HTTPException) as ctx:
            self._run(rooms_routes.create_room(body=body, current_user=user2))
        self.assertEqual(ctx.exception.status_code, 403)


class TestGetRoom(RoomsTestCase):
    def test_get_room_not_found(self) -> None:
        user = _fake_user()
        with self.assertRaises(HTTPException) as ctx:
            rooms_routes.get_room(room_id="no-such-room", current_user=user)
        self.assertEqual(ctx.exception.status_code, 404)

    def test_get_room_success(self) -> None:
        _seed_room(self.conn, room_id="room-1", name="general", owner_id="user-1")
        user = _fake_user()
        result = rooms_routes.get_room(room_id="room-1", current_user=user)
        self.assertEqual(result.id, "room-1")
        self.assertEqual(result.name, "general")


class TestDeleteRoom(RoomsTestCase):
    def test_delete_room_by_creator(self) -> None:
        _seed_room(self.conn, room_id="room-1", name="general", owner_id="user-1")
        user = _fake_user()
        result = self._run(rooms_routes.delete_room(room_id="room-1", current_user=user))
        self.assertEqual(result, {})
        row = self.conn.execute("SELECT id FROM rooms WHERE id = 'room-1'").fetchone()
        self.assertIsNone(row)

    def test_delete_room_by_non_creator(self) -> None:
        _seed_room(self.conn, room_id="room-1", name="general", owner_id="user-1")
        _seed_user(self.conn, uid="user-2", username="bob")
        user2 = _fake_user(uid="user-2", username="bob")
        with self.assertRaises(HTTPException) as ctx:
            self._run(rooms_routes.delete_room(room_id="room-1", current_user=user2))
        self.assertEqual(ctx.exception.status_code, 403)

    def test_delete_room_not_found(self) -> None:
        user = _fake_user()
        with self.assertRaises(HTTPException) as ctx:
            self._run(rooms_routes.delete_room(room_id="ghost-room", current_user=user))
        self.assertEqual(ctx.exception.status_code, 404)


# ---------------------------------------------------------------------------
# Tests: bots
# ---------------------------------------------------------------------------

class TestAddBot(RoomsTestCase):
    def test_add_bot_room_not_found(self) -> None:
        user = _fake_user()
        body = BotConfigCreate(name="mybot", provider=BotProvider.CLAUDE)
        with self.assertRaises(HTTPException) as ctx:
            rooms_routes.add_bot(room_id="no-room", body=body, current_user=user)
        self.assertEqual(ctx.exception.status_code, 404)

    def test_add_bot_only_creator(self) -> None:
        _seed_room(self.conn, room_id="room-1", name="general", owner_id="user-1")
        _seed_user(self.conn, uid="user-2", username="bob")
        user2 = _fake_user(uid="user-2", username="bob")
        body = BotConfigCreate(name="mybot", provider=BotProvider.CLAUDE)
        with self.assertRaises(HTTPException) as ctx:
            rooms_routes.add_bot(room_id="room-1", body=body, current_user=user2)
        self.assertEqual(ctx.exception.status_code, 403)

    def test_add_bot_duplicate(self) -> None:
        _seed_room(self.conn, room_id="room-1", name="general", owner_id="user-1")
        _seed_bot(self.conn, bot_id="bot-1", room_id="room-1", name="mybot")
        user = _fake_user()
        body = BotConfigCreate(name="mybot", provider=BotProvider.CLAUDE)
        with self.assertRaises(HTTPException) as ctx:
            rooms_routes.add_bot(room_id="room-1", body=body, current_user=user)
        self.assertEqual(ctx.exception.status_code, 409)

    def test_add_bot_success(self) -> None:
        _seed_room(self.conn, room_id="room-1", name="general", owner_id="user-1")
        user = _fake_user()
        body = BotConfigCreate(name="helper", provider=BotProvider.CLAUDE, personality=BotPersonality.ASSISTANT)
        result = rooms_routes.add_bot(room_id="room-1", body=body, current_user=user)
        self.assertEqual(result.name, "helper")
        self.assertEqual(result.room_id, "room-1")
        self.assertEqual(result.provider, BotProvider.CLAUDE)
        self.assertIsNotNone(result.id)


class TestUpdateBot(RoomsTestCase):
    def test_update_bot_success(self) -> None:
        _seed_room(self.conn, room_id="room-1", name="general", owner_id="user-1")
        _seed_bot(self.conn, bot_id="bot-1", room_id="room-1", name="mybot", added_by="user-1")
        user = _fake_user()
        body = BotConfigUpdate(personality=BotPersonality.CODER)
        result = rooms_routes.update_bot(room_id="room-1", bot_id="bot-1", body=body, current_user=user)
        self.assertEqual(result.personality, BotPersonality.CODER)


class TestDeleteBot(RoomsTestCase):
    def test_delete_bot_success(self) -> None:
        _seed_room(self.conn, room_id="room-1", name="general", owner_id="user-1")
        _seed_bot(self.conn, bot_id="bot-1", room_id="room-1", name="mybot", added_by="user-1")
        user = _fake_user()
        result = rooms_routes.delete_bot(room_id="room-1", bot_id="bot-1", current_user=user)
        self.assertEqual(result, {})
        row = self.conn.execute("SELECT id FROM room_bots WHERE id = 'bot-1'").fetchone()
        self.assertIsNone(row)

    def test_delete_bot_not_found(self) -> None:
        _seed_room(self.conn, room_id="room-1", name="general", owner_id="user-1")
        user = _fake_user()
        with self.assertRaises(HTTPException) as ctx:
            rooms_routes.delete_bot(room_id="room-1", bot_id="no-such-bot", current_user=user)
        self.assertEqual(ctx.exception.status_code, 404)

    def test_delete_bot_room_not_found(self) -> None:
        user = _fake_user()
        with self.assertRaises(HTTPException) as ctx:
            rooms_routes.delete_bot(room_id="ghost-room", bot_id="bot-1", current_user=user)
        self.assertEqual(ctx.exception.status_code, 404)

    def test_delete_bot_non_creator(self) -> None:
        _seed_room(self.conn, room_id="room-1", name="general", owner_id="user-1")
        _seed_bot(self.conn, bot_id="bot-1", room_id="room-1", name="mybot", added_by="user-1")
        _seed_user(self.conn, uid="user-2", username="bob")
        user2 = _fake_user(uid="user-2", username="bob")
        with self.assertRaises(HTTPException) as ctx:
            rooms_routes.delete_bot(room_id="room-1", bot_id="bot-1", current_user=user2)
        self.assertEqual(ctx.exception.status_code, 403)


class TestListRoomsWithServerFilter(RoomsTestCase):
    def test_list_rooms_by_server_id(self) -> None:
        _seed_server(self.conn, server_id="srv-1", owner_id="user-1")
        _seed_room(self.conn, room_id="room-1", name="general", owner_id="user-1", server_id="srv-1")
        _seed_room(self.conn, room_id="room-2", name="random", owner_id="user-1", server_id=None)
        user = _fake_user()
        result = rooms_routes.list_rooms(server_id="srv-1", current_user=user)
        self.assertEqual(len(result), 1)
        self.assertEqual(result[0].id, "room-1")


class TestCreateRoomWithServer(RoomsTestCase):
    def test_create_room_in_server_broadcasts_server_channel(self) -> None:
        _seed_server(self.conn, server_id="srv-1", owner_id="user-1")
        self.conn.execute(
            "INSERT OR IGNORE INTO server_members (server_id, user_id, username, joined_at) VALUES (?, ?, ?, ?)",
            ("srv-1", "user-1", "alice", "2024-01-01T00:00:00Z"),
        )
        self.conn.commit()
        user = _fake_user()
        body = RoomCreate(name="new-channel", server_id="srv-1")
        result = self._run(rooms_routes.create_room(body=body, current_user=user))
        self.assertEqual(result.server_id, "srv-1")
        # Should have called publish twice (server + global)
        self.assertEqual(self.mock_publish.call_count, 2)


class TestUpdateBotAdditional(RoomsTestCase):
    def test_update_bot_room_not_found(self) -> None:
        user = _fake_user()
        body = BotConfigUpdate(personality=BotPersonality.CODER)
        with self.assertRaises(HTTPException) as ctx:
            rooms_routes.update_bot(room_id="ghost-room", bot_id="bot-1", body=body, current_user=user)
        self.assertEqual(ctx.exception.status_code, 404)

    def test_update_bot_non_creator(self) -> None:
        _seed_room(self.conn, room_id="room-1", name="general", owner_id="user-1")
        _seed_bot(self.conn, bot_id="bot-1", room_id="room-1", name="mybot", added_by="user-1")
        _seed_user(self.conn, uid="user-2", username="bob")
        user2 = _fake_user(uid="user-2", username="bob")
        body = BotConfigUpdate(personality=BotPersonality.CODER)
        with self.assertRaises(HTTPException) as ctx:
            rooms_routes.update_bot(room_id="room-1", bot_id="bot-1", body=body, current_user=user2)
        self.assertEqual(ctx.exception.status_code, 403)

    def test_update_bot_not_found(self) -> None:
        _seed_room(self.conn, room_id="room-1", name="general", owner_id="user-1")
        user = _fake_user()
        body = BotConfigUpdate(personality=BotPersonality.CODER)
        with self.assertRaises(HTTPException) as ctx:
            rooms_routes.update_bot(room_id="room-1", bot_id="ghost-bot", body=body, current_user=user)
        self.assertEqual(ctx.exception.status_code, 404)

    def test_update_bot_no_changes(self) -> None:
        _seed_room(self.conn, room_id="room-1", name="general", owner_id="user-1")
        _seed_bot(self.conn, bot_id="bot-1", room_id="room-1", name="mybot", added_by="user-1")
        user = _fake_user()
        body = BotConfigUpdate()  # no fields set
        result = rooms_routes.update_bot(room_id="room-1", bot_id="bot-1", body=body, current_user=user)
        self.assertEqual(result.id, "bot-1")


class TestDeleteRoomWithServer(RoomsTestCase):
    def test_delete_room_in_server_by_server_owner(self) -> None:
        _seed_server(self.conn, server_id="srv-1", owner_id="user-1")
        _seed_user(self.conn, uid="user-2", username="bob")
        _seed_room(self.conn, room_id="room-1", name="general", owner_id="user-2", server_id="srv-1")
        user = _fake_user()  # user-1 is server owner
        result = self._run(rooms_routes.delete_room(room_id="room-1", current_user=user))
        self.assertEqual(result, {})

    def test_delete_room_in_server_by_non_server_owner(self) -> None:
        _seed_server(self.conn, server_id="srv-1", owner_id="user-1")
        _seed_room(self.conn, room_id="room-1", name="general", owner_id="user-1", server_id="srv-1")
        _seed_user(self.conn, uid="user-2", username="bob")
        user2 = _fake_user(uid="user-2", username="bob")
        with self.assertRaises(HTTPException) as ctx:
            self._run(rooms_routes.delete_room(room_id="room-1", current_user=user2))
        self.assertEqual(ctx.exception.status_code, 403)


class TestHelperFunctions(RoomsTestCase):
    def test_server_channel_format(self) -> None:
        result = rooms_routes._server_channel("srv-abc")
        self.assertEqual(result, "server:srv-abc")

    def test_get_bots_for_room_empty(self) -> None:
        _seed_room(self.conn, room_id="room-1", name="general", owner_id="user-1")
        result = rooms_routes._get_bots_for_room(self.conn, "room-1")
        self.assertEqual(result, [])

    def test_get_bots_for_room_with_bot(self) -> None:
        _seed_room(self.conn, room_id="room-1", name="general", owner_id="user-1")
        _seed_bot(self.conn, bot_id="bot-1", room_id="room-1", name="mybot", added_by="user-1")
        result = rooms_routes._get_bots_for_room(self.conn, "room-1")
        self.assertEqual(len(result), 1)
        self.assertEqual(result[0].name, "mybot")
