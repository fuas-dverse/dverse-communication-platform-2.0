"""Unit tests for backend/routes/servers.py.

Approach:
- In-memory SQLite provisioned via init_db().
- backend.db.get_db and backend.routes.servers.get_db are patched to return
  the same in-memory connection.
- broker.publish is patched as AsyncMock.
- Async route functions are driven with asyncio.run().
"""

import asyncio
import sqlite3
import unittest
from unittest.mock import AsyncMock, patch

from fastapi import HTTPException

import backend.db as db_module
import backend.routes.servers as servers_routes
from backend.models.server import ServerCreate
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


def _seed_server(
    conn: sqlite3.Connection,
    server_id: str = "srv-1",
    name: str = "myserver",
    owner_id: str = "user-1",
    invite_code: str | None = None,
) -> None:
    conn.execute(
        "INSERT OR IGNORE INTO servers (id, name, description, created_by, created_at, invite_code) VALUES (?, ?, ?, ?, ?, ?)",
        (server_id, name, "", owner_id, "2024-01-01T00:00:00Z", invite_code),
    )
    conn.commit()


def _seed_member(
    conn: sqlite3.Connection,
    server_id: str = "srv-1",
    user_id: str = "user-1",
    username: str = "alice",
) -> None:
    conn.execute(
        "INSERT OR IGNORE INTO server_members (server_id, user_id, username, joined_at) VALUES (?, ?, ?, ?)",
        (server_id, user_id, username, "2024-01-01T00:00:00Z"),
    )
    conn.commit()


# ---------------------------------------------------------------------------
# Base test class
# ---------------------------------------------------------------------------

class ServersTestCase(unittest.TestCase):
    def setUp(self) -> None:
        self.conn = make_test_db()

        db_patcher = patch.object(db_module, "get_db", return_value=self.conn)
        routes_patcher = patch("backend.routes.servers.get_db", return_value=self.conn)
        broker_patcher = patch("backend.routes.servers.broker.publish", new_callable=AsyncMock)

        self.mock_db = db_patcher.start()
        self.mock_routes_db = routes_patcher.start()
        self.mock_publish = broker_patcher.start()

        self.addCleanup(db_patcher.stop)
        self.addCleanup(routes_patcher.stop)
        self.addCleanup(broker_patcher.stop)

        db_module.init_db()
        _seed_user(self.conn)

    def tearDown(self) -> None:
        self.conn.close()

    def _run(self, coro):
        return asyncio.run(coro)


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

class TestListServers(ServersTestCase):
    def test_list_servers_empty(self) -> None:
        user = _fake_user()
        result = servers_routes.list_servers(current_user=user)
        self.assertEqual(result, [])


class TestCreateServer(ServersTestCase):
    def test_create_server_success(self) -> None:
        user = _fake_user()
        body = ServerCreate(name="awesome-server", description="cool place")
        result = self._run(servers_routes.create_server(body=body, current_user=user))
        self.assertEqual(result.name, "awesome-server")
        self.assertEqual(result.description, "cool place")
        self.assertEqual(result.created_by, user.id)
        self.assertEqual(result.member_count, 1)

    def test_create_server_duplicate(self) -> None:
        user = _fake_user()
        body = ServerCreate(name="awesome-server")
        self._run(servers_routes.create_server(body=body, current_user=user))
        with self.assertRaises(HTTPException) as ctx:
            self._run(servers_routes.create_server(body=body, current_user=user))
        self.assertEqual(ctx.exception.status_code, 409)


class TestJoinServer(ServersTestCase):
    def test_join_server_success(self) -> None:
        _seed_server(self.conn, server_id="srv-1", name="myserver", owner_id="user-1")
        _seed_user(self.conn, uid="user-2", username="bob")
        user2 = _fake_user(uid="user-2", username="bob")
        result = self._run(servers_routes.join_server(server_id="srv-1", current_user=user2))
        self.assertEqual(result.id, "srv-1")
        row = self.conn.execute(
            "SELECT 1 FROM server_members WHERE server_id = 'srv-1' AND user_id = 'user-2'"
        ).fetchone()
        self.assertIsNotNone(row)

    def test_join_server_not_found(self) -> None:
        user = _fake_user()
        with self.assertRaises(HTTPException) as ctx:
            self._run(servers_routes.join_server(server_id="ghost-srv", current_user=user))
        self.assertEqual(ctx.exception.status_code, 404)


class TestLeaveServer(ServersTestCase):
    def test_leave_server_as_owner(self) -> None:
        _seed_server(self.conn, server_id="srv-1", name="myserver", owner_id="user-1")
        _seed_member(self.conn, server_id="srv-1", user_id="user-1", username="alice")
        user = _fake_user()
        with self.assertRaises(HTTPException) as ctx:
            self._run(servers_routes.leave_server(server_id="srv-1", current_user=user))
        self.assertEqual(ctx.exception.status_code, 400)

    def test_leave_server_success(self) -> None:
        _seed_server(self.conn, server_id="srv-1", name="myserver", owner_id="user-1")
        _seed_user(self.conn, uid="user-2", username="bob")
        _seed_member(self.conn, server_id="srv-1", user_id="user-2", username="bob")
        user2 = _fake_user(uid="user-2", username="bob")
        result = self._run(servers_routes.leave_server(server_id="srv-1", current_user=user2))
        self.assertEqual(result, {})


class TestGenerateInvite(ServersTestCase):
    def test_generate_invite_non_owner(self) -> None:
        _seed_server(self.conn, server_id="srv-1", name="myserver", owner_id="user-1")
        _seed_user(self.conn, uid="user-2", username="bob")
        _seed_member(self.conn, server_id="srv-1", user_id="user-2", username="bob")
        user2 = _fake_user(uid="user-2", username="bob")
        with self.assertRaises(HTTPException) as ctx:
            servers_routes.generate_invite(server_id="srv-1", current_user=user2)
        self.assertEqual(ctx.exception.status_code, 403)

    def test_generate_invite_success(self) -> None:
        _seed_server(self.conn, server_id="srv-1", name="myserver", owner_id="user-1")
        _seed_member(self.conn, server_id="srv-1", user_id="user-1", username="alice")
        user = _fake_user()
        result = servers_routes.generate_invite(server_id="srv-1", current_user=user)
        self.assertIsNotNone(result.invite_code)
        self.assertIsInstance(result.invite_code, str)
        self.assertGreater(len(result.invite_code), 0)


class TestJoinByInvite(ServersTestCase):
    def test_join_by_invite_invalid_code(self) -> None:
        user = _fake_user()
        with self.assertRaises(HTTPException) as ctx:
            self._run(servers_routes.join_by_invite(invite_code="BADCODE", current_user=user))
        self.assertEqual(ctx.exception.status_code, 404)

    def test_join_by_invite_success(self) -> None:
        _seed_server(self.conn, server_id="srv-1", name="myserver", owner_id="user-1", invite_code="ABCD1234")
        _seed_user(self.conn, uid="user-2", username="bob")
        user2 = _fake_user(uid="user-2", username="bob")
        result = self._run(servers_routes.join_by_invite(invite_code="ABCD1234", current_user=user2))
        self.assertEqual(result.id, "srv-1")
        row = self.conn.execute(
            "SELECT 1 FROM server_members WHERE server_id = 'srv-1' AND user_id = 'user-2'"
        ).fetchone()
        self.assertIsNotNone(row)


class TestGetMembers(ServersTestCase):
    def test_get_members_not_member(self) -> None:
        _seed_server(self.conn, server_id="srv-1", name="myserver", owner_id="user-1")
        _seed_user(self.conn, uid="user-2", username="bob")
        user2 = _fake_user(uid="user-2", username="bob")
        with self.assertRaises(HTTPException) as ctx:
            self._run(servers_routes.get_members(server_id="srv-1", current_user=user2))
        self.assertEqual(ctx.exception.status_code, 403)

    def test_get_members_success(self) -> None:
        _seed_server(self.conn, server_id="srv-1", name="myserver", owner_id="user-1")
        _seed_member(self.conn, server_id="srv-1", user_id="user-1", username="alice")
        user = _fake_user()
        result = self._run(servers_routes.get_members(server_id="srv-1", current_user=user))
        self.assertIsInstance(result, list)
        self.assertEqual(len(result), 1)
        self.assertEqual(result[0].user_id, "user-1")


class TestListServersWithMember(ServersTestCase):
    def test_list_servers_with_member(self) -> None:
        _seed_server(self.conn, server_id="srv-1", name="myserver", owner_id="user-1")
        _seed_member(self.conn, server_id="srv-1", user_id="user-1", username="alice")
        user = _fake_user()
        result = servers_routes.list_servers(current_user=user)
        self.assertEqual(len(result), 1)
        self.assertEqual(result[0].name, "myserver")
        self.assertEqual(result[0].member_count, 1)


class TestCreateServerAutoJoins(ServersTestCase):
    def test_create_server_auto_joins_creator(self) -> None:
        user = _fake_user()
        body = ServerCreate(name="new-server")
        result = self._run(servers_routes.create_server(body=body, current_user=user))
        row = self.conn.execute(
            "SELECT 1 FROM server_members WHERE server_id = ? AND user_id = ?",
            (result.id, user.id),
        ).fetchone()
        self.assertIsNotNone(row)


class TestJoinServerAlreadyMember(ServersTestCase):
    def test_join_server_already_member(self) -> None:
        _seed_server(self.conn, server_id="srv-1", name="myserver", owner_id="user-1")
        _seed_member(self.conn, server_id="srv-1", user_id="user-1", username="alice")
        user = _fake_user()
        # Join when already a member — should be idempotent
        result = self._run(servers_routes.join_server(server_id="srv-1", current_user=user))
        self.assertEqual(result.id, "srv-1")
        # Should have broadcast member_online
        self.mock_publish.assert_called()


class TestJoinByInviteAlreadyMember(ServersTestCase):
    def test_join_by_invite_already_member(self) -> None:
        _seed_server(self.conn, server_id="srv-1", name="myserver", owner_id="user-1", invite_code="TESTCODE")
        _seed_member(self.conn, server_id="srv-1", user_id="user-1", username="alice")
        user = _fake_user()
        result = self._run(servers_routes.join_by_invite(invite_code="TESTCODE", current_user=user))
        self.assertEqual(result.id, "srv-1")
        self.mock_publish.assert_called()


class TestGenerateInviteRegenerates(ServersTestCase):
    def test_generate_invite_regenerates_code(self) -> None:
        _seed_server(self.conn, server_id="srv-1", name="myserver", owner_id="user-1")
        _seed_member(self.conn, server_id="srv-1", user_id="user-1", username="alice")
        user = _fake_user()
        result1 = servers_routes.generate_invite(server_id="srv-1", current_user=user)
        result2 = servers_routes.generate_invite(server_id="srv-1", current_user=user)
        # Both codes are non-empty valid strings
        self.assertIsNotNone(result1.invite_code)
        self.assertIsNotNone(result2.invite_code)
        self.assertGreater(len(result1.invite_code), 0)
        self.assertGreater(len(result2.invite_code), 0)


class TestPresenceFunctions(ServersTestCase):
    def test_list_user_server_ids(self) -> None:
        _seed_server(self.conn, server_id="srv-1", name="myserver", owner_id="user-1")
        _seed_member(self.conn, server_id="srv-1", user_id="user-1", username="alice")
        result = servers_routes._list_user_server_ids("user-1")
        self.assertIn("srv-1", result)

    def test_list_user_server_ids_empty(self) -> None:
        result = servers_routes._list_user_server_ids("nobody")
        self.assertEqual(result, [])

    def test_mark_member_online_broadcasts(self) -> None:
        _seed_server(self.conn, server_id="srv-1", name="myserver", owner_id="user-1")
        _seed_member(self.conn, server_id="srv-1", user_id="user-1", username="alice")
        user = _fake_user()
        # Reset presence state
        servers_routes._presence_connections.clear()
        self._run(servers_routes._mark_member_online("srv-1", user))
        # Should have published online event
        self.mock_publish.assert_called()
        # DB should show is_online=1
        row = self.conn.execute(
            "SELECT is_online FROM server_members WHERE server_id='srv-1' AND user_id='user-1'"
        ).fetchone()
        self.assertEqual(row["is_online"], 1)

    def test_mark_member_offline_broadcasts(self) -> None:
        _seed_server(self.conn, server_id="srv-1", name="myserver", owner_id="user-1")
        _seed_member(self.conn, server_id="srv-1", user_id="user-1", username="alice")
        user = _fake_user()
        servers_routes._presence_connections.clear()
        servers_routes._presence_connections[user.id] = 1
        self._run(servers_routes._mark_member_offline_if_last_connection("srv-1", user))
        self.mock_publish.assert_called()

    def test_broadcast_user_presence_online(self) -> None:
        _seed_server(self.conn, server_id="srv-1", name="myserver", owner_id="user-1")
        _seed_member(self.conn, server_id="srv-1", user_id="user-1", username="alice")
        self._run(servers_routes._broadcast_user_presence("user-1", is_online=True))
        self.mock_publish.assert_called()

    def test_broadcast_user_presence_offline(self) -> None:
        _seed_server(self.conn, server_id="srv-1", name="myserver", owner_id="user-1")
        _seed_member(self.conn, server_id="srv-1", user_id="user-1", username="alice")
        self._run(servers_routes._broadcast_user_presence("user-1", is_online=False, last_seen="2024-01-01T00:00:00Z"))
        self.mock_publish.assert_called()

    def test_touch_member_presence(self) -> None:
        user = _fake_user()
        # Should not raise
        self._run(servers_routes._touch_member_presence("srv-1", user))

    def test_leave_server_not_found(self) -> None:
        user = _fake_user()
        with self.assertRaises(Exception):
            self._run(servers_routes.leave_server(server_id="ghost-srv", current_user=user))


class TestHelperFunctions(ServersTestCase):
    def test_is_member_online_true_recent(self) -> None:
        from datetime import datetime, timezone
        now_iso = datetime.now(timezone.utc).isoformat()
        result = servers_routes._is_member_online(True, now_iso)
        self.assertTrue(result)

    def test_is_member_online_false_flag(self) -> None:
        from datetime import datetime, timezone
        now_iso = datetime.now(timezone.utc).isoformat()
        result = servers_routes._is_member_online(False, now_iso)
        self.assertFalse(result)

    def test_is_member_online_old_timestamp(self) -> None:
        result = servers_routes._is_member_online(True, "2000-01-01T00:00:00+00:00")
        self.assertFalse(result)

    def test_is_member_online_none_last_seen(self) -> None:
        result = servers_routes._is_member_online(True, None)
        self.assertFalse(result)

    def test_utc_now_returns_aware_datetime(self) -> None:
        from datetime import timezone
        dt = servers_routes._utc_now()
        self.assertIsNotNone(dt.tzinfo)
        self.assertEqual(dt.tzinfo, timezone.utc)

    def test_row_to_server_helper(self) -> None:
        _seed_server(self.conn, server_id="srv-1", name="myserver", owner_id="user-1")
        _seed_member(self.conn, server_id="srv-1", user_id="user-1", username="alice")
        row = self.conn.execute("SELECT * FROM servers WHERE id = 'srv-1'").fetchone()
        server = servers_routes._row_to_server(self.conn, row)
        self.assertEqual(server.id, "srv-1")
        self.assertEqual(server.name, "myserver")
        self.assertEqual(server.member_count, 1)
