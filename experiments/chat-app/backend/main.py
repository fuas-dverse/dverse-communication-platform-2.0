import asyncio

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from dotenv import load_dotenv
from .db import init_db
from .routes import auth, rooms, messages, servers

load_dotenv()

app = FastAPI(title="ChatApp API")

app.add_middleware(
    CORSMiddleware,
    allow_origins=["http://localhost:5173"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


@app.on_event("startup")
async def startup():
    init_db()
    from .services.zenoh_bridge import zenoh_bridge
    loop = asyncio.get_event_loop()
    await loop.run_in_executor(None, lambda: zenoh_bridge.start(loop))


@app.on_event("shutdown")
async def shutdown():
    from .services.zenoh_bridge import zenoh_bridge
    zenoh_bridge.close()


app.include_router(auth.router, prefix="/auth", tags=["auth"])
app.include_router(servers.router, prefix="/servers", tags=["servers"])
app.include_router(rooms.router, prefix="/rooms", tags=["rooms"])
app.include_router(messages.router, prefix="/rooms", tags=["messages"])
