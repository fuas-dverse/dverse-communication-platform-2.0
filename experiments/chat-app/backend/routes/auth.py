import uuid
from datetime import datetime

from fastapi import APIRouter, Depends, HTTPException, status

from ..auth import create_token, get_current_user, hash_password, verify_password
from ..db import get_db
from ..models.user import TokenResponse, User, UserCreate, UserLogin

router = APIRouter()


@router.post("/register", response_model=TokenResponse, status_code=status.HTTP_201_CREATED)
def register(body: UserCreate):
    db = get_db()

    existing = db.execute(
        "SELECT id FROM users WHERE username = ?", (body.username,)
    ).fetchone()
    if existing:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail="Username already taken",
        )

    user_id = str(uuid.uuid4())
    created_at = datetime.utcnow().isoformat() + "Z"
    password_hash = hash_password(body.password)

    db.execute(
        "INSERT INTO users (id, username, password_hash, created_at) VALUES (?, ?, ?, ?)",
        (user_id, body.username, password_hash, created_at),
    )
    db.commit()

    user = User(id=user_id, username=body.username, created_at=created_at)
    token = create_token(user_id)

    return TokenResponse(access_token=token, user=user)


@router.post("/login", response_model=TokenResponse)
def login(body: UserLogin):
    db = get_db()

    row = db.execute(
        "SELECT id, username, password_hash, created_at FROM users WHERE username = ?",
        (body.username,),
    ).fetchone()

    if not row or not verify_password(body.password, row["password_hash"]):
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid username or password",
        )

    user = User(id=row["id"], username=row["username"], created_at=row["created_at"])
    token = create_token(row["id"])

    return TokenResponse(access_token=token, user=user)


@router.get("/me", response_model=User)
def me(current_user: User = Depends(get_current_user)):
    return current_user
