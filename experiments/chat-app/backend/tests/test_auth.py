import unittest
from fastapi import HTTPException
from fastapi.security import HTTPAuthorizationCredentials
from jose import jwt

import backend.routes.auth as auth_routes
import backend.auth as auth_module
from backend.models.user import UserCreate, UserLogin


class TestAuth(unittest.TestCase):
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

    def test_get_current_user_rejects_missing_user(self) -> None:
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