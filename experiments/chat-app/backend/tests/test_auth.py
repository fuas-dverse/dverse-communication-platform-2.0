import os
import sqlite3
import unittest
from unittest.mock import patch

from fastapi import HTTPException
from fastapi.security import HTTPAuthorizationCredentials
from jose import jwt

import backend.auth as auth_module
import backend.routes.auth as auth_routes
from backend.models.user import UserCreate, UserLogin


def make_test_db() -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:", check_same_thread=False)
    conn.execute("PRAGMA foreign_keys=ON")
    conn.row_factory = sqlite3.Row
    conn.executescript("""
        CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            username TEXT UNIQUE NOT NULL,
            password_hash TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
    """)
    conn.commit()
    return conn


class TestAuth(unittest.TestCase):
    def setUp(self) -> None:
        self.conn = make_test_db()

        routes_patcher = patch("backend.routes.auth.get_db", return_value=self.conn)
        auth_patcher = patch("backend.auth.get_db", return_value=self.conn)

        self.mock_routes_get_db = routes_patcher.start()
        self.mock_auth_get_db = auth_patcher.start()

        self.addCleanup(routes_patcher.stop)
        self.addCleanup(auth_patcher.stop)

    def tearDown(self) -> None:
        self.conn.close()

    def test_register_duplicate_user_fails(self) -> None:
        auth_routes.register(UserCreate(username="bob", password="secret"))

        with self.assertRaises(HTTPException) as context:
            auth_routes.register(UserCreate(username="bob", password="another-secret"))

        self.assertEqual(context.exception.status_code, 409)

    def test_login_fails_with_wrong_password(self) -> None:
        auth_routes.register(UserCreate(username="bob", password="secret"))

        with self.assertRaises(HTTPException) as context:
            auth_routes.login(UserLogin(username="bob", password="wrong-password"))

        self.assertEqual(context.exception.status_code, 401)

    def test_get_current_user_rejects_token_signed_with_wrong_secret(self) -> None:
        auth_routes.register(UserCreate(username="carol", password="secret"))

        invalid_token = jwt.encode(
            {"sub": "carol"},
            "different-secret",
            algorithm=auth_module.ALGORITHM,
        )

        credentials = HTTPAuthorizationCredentials(
            scheme="Bearer",
            credentials=invalid_token,
        )

        with self.assertRaises(HTTPException) as context:
            auth_module.get_current_user(credentials)

        self.assertEqual(context.exception.status_code, 401)

    def test_get_current_user_rejects_missing_user_2(self) -> None:
        token = jwt.encode(
            {"sub": "missing-user"},
            "unit-test-secret",
            algorithm=auth_module.ALGORITHM,
        )

        credentials = HTTPAuthorizationCredentials(
            scheme="Bearer",
            credentials=token,
        )

        with self.assertRaises(HTTPException) as context:
            auth_module.get_current_user(credentials)

        self.assertEqual(context.exception.status_code, 401)

    def test_register_success(self) -> None:
        result = auth_routes.register(UserCreate(username="dave", password="pass123"))
        self.assertIsNotNone(result.access_token)
        self.assertEqual(result.user.username, "dave")

    def test_login_success(self) -> None:
        auth_routes.register(UserCreate(username="eve", password="mypassword"))
        result = auth_routes.login(UserLogin(username="eve", password="mypassword"))
        self.assertIsNotNone(result.access_token)
        self.assertEqual(result.user.username, "eve")

    def test_get_current_user_success(self) -> None:
        reg = auth_routes.register(UserCreate(username="frank", password="pass"))
        credentials = HTTPAuthorizationCredentials(
            scheme="Bearer",
            credentials=reg.access_token,
        )
        with patch("backend.auth.get_db", return_value=self.conn):
            user = auth_module.get_current_user(credentials)
        self.assertEqual(user.username, "frank")