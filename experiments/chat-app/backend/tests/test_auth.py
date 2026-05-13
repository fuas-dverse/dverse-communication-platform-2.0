import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from fastapi import HTTPException
from fastapi.security import HTTPAuthorizationCredentials
from jose import jwt

from backend import auth as auth_module
from backend import db as db_module
from backend.models.user import UserCreate, UserLogin
from backend.routes import auth as auth_routes


class AuthTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.database_path = Path(self.temp_dir.name) / "chatapp-test.db"

        self.db_path_patch = patch.object(db_module, "DB_PATH", self.database_path)
        self.db_path_patch.start()

        if hasattr(db_module._local, "conn"):
            delattr(db_module._local, "conn")

        db_module.init_db()
        self.connection = db_module.get_db()

        self.secret_patch = patch.dict(os.environ, {"SECRET_KEY": "unit-test-secret"}, clear=False)
        self.secret_patch.start()

        self.route_db_patch = patch.object(auth_routes, "get_db", return_value=self.connection)
        self.route_db_patch.start()

        self.auth_db_patch = patch.object(auth_module, "get_db", return_value=self.connection)
        self.auth_db_patch.start()

    def tearDown(self) -> None:
        self.auth_db_patch.stop()
        self.route_db_patch.stop()
        self.secret_patch.stop()

        self.connection.close()

        if hasattr(db_module._local, "conn"):
            delattr(db_module._local, "conn")

        self.db_path_patch.stop()
        self.temp_dir.cleanup()

    def test_password_helpers_and_token_creation(self) -> None:
        hashed = auth_module.hash_password("secret-password")

        self.assertTrue(auth_module.verify_password("secret-password", hashed))
        self.assertFalse(auth_module.verify_password("wrong-password", hashed))

        token = auth_module.create_token("user-123")
        payload = jwt.decode(token, "unit-test-secret", algorithms=[auth_module.ALGORITHM])

        self.assertEqual(payload["sub"], "user-123")

    def test_register_login_and_current_user(self) -> None:
        created = auth_routes.register(UserCreate(username="alice", password="secret"))

        self.assertEqual(created.user.username, "alice")
        self.assertEqual(created.token_type, "bearer")

        login_response = auth_routes.login(UserLogin(username="alice", password="secret"))
        self.assertEqual(login_response.user.id, created.user.id)

        credentials = HTTPAuthorizationCredentials(scheme="Bearer", credentials=created.access_token)
        current_user = auth_module.get_current_user(credentials)
        self.assertEqual(current_user.id, created.user.id)

        with self.assertRaises(HTTPException) as context:
            auth_routes.register(UserCreate(username="alice", password="another-secret"))

        self.assertEqual(context.exception.status_code, 409)

 auth_routes.register(UserCreate(username="bob", password="secret"))

         with self.assertRaises(HTTPException) as context:
         auth_routes.login(UserLogin(username="bob", password="wrong-password"))

         self.assertEqual(context.exception.status_code,401)

         def test_get_current_user_rejects_token_signed_with_wrong_secret(self) -> None:
         auth_routes.register(UserCreate(username="carol", password="secret"))

         invalid_token = jwt.encode(
         {"sub": "carol"},
         "different-secret",
         algorithm=auth_module.ALGORITHM,
         )
         credentials = HTTPAuthorizationCredentials(scheme="Bearer", credentials=invalid_token)

         with self.assertRaises(HTTPException) as context:
         auth_module.get_current_user(credentials)

         self.assertEqual(context.exception.status_code,401)

         def test_get_current_user_rejects_token_for_missing_user(self) -> None:
         token = jwt.encode(
         {"sub": "missing-user"},
         "unit-test-secret",
         algorithm=auth_module.ALGORITHM,
         )
         credentials = HTTPAuthorizationCredentials(scheme="Bearer", credentials=token)

         with self.assertRaises(HTTPException) as context:
         auth_module.get_current_user(credentials)

         self.assertEqual(context.exception.status_code,401)

         auth_routes.register(UserCreate(username="bob", password="secret"))

         with self.assertRaises(HTTPException) as context:
         auth_routes.login(UserLogin(username="bob", password="wrong-password"))

         self.assertEqual(context.exception.status_code,401)

         def test_get_current_user_rejects_token_signed_with_wrong_secret(self) -> None:
         auth_routes.register(UserCreate(username="carol", password="secret"))

         invalid_token = jwt.encode(
         {"sub": "carol"},
         "different-secret",
         algorithm=auth_module.ALGORITHM,
         )
         credentials = HTTPAuthorizationCredentials(scheme="Bearer", credentials=invalid_token)

         with self.assertRaises(HTTPException) as context:
         auth_module.get_current_user(credentials)

         self.assertEqual(context.exception.status_code,401)

         def test_get_current_user_rejects_token_for_missing_user(self) -> None:
         token = jwt.encode(
         {"sub": "missing-user"},
         "unit-test-secret",
         algorithm=auth_module.ALGORITHM,
         )
         credentials = HTTPAuthorizationCredentials(scheme="Bearer", credentials=token)

         with self.assertRaises(HTTPException) as context:
         auth_module.get_current_user(credentials)

         self.assertEqual(context.exception.status_code,401)
