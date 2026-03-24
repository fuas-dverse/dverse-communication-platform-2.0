"""
Bridges asyncio (FastAPI) and Zenoh (sync/threaded).

Request flow:
  1. Backend publishes  → chat/{room_id}/request/{bot_name}
     payload: { request_id, room_id, bot_name, message, history }

  2. Bot agent receives → calls its LLM → publishes
     chat/response/{request_id}
     payload: { request_id, content }

  3. Bridge subscriber receives response → resolves the waiting Future
"""

import asyncio
import json
import os
import uuid
from typing import Optional


class ZenohBridge:
    def __init__(self):
        self._session = None
        self._sub = None
        self._pending: dict[str, asyncio.Future] = {}
        self._loop: Optional[asyncio.AbstractEventLoop] = None
        self.available = False

    def start(self, loop: asyncio.AbstractEventLoop):
        self._loop = loop
        router = os.environ.get("ZENOH_ROUTER", "tcp/localhost:7447")
        try:
            import zenoh  # optional dependency
            conf = zenoh.Config()
            conf.insert_json5("connect/endpoints", json.dumps([router]))
            self._session = zenoh.open(conf)
            self._sub = self._session.declare_subscriber(
                "chat/response/**",
                self._on_response,
            )
            self.available = True
            print(f"[Zenoh] Connected to router at {router}")
        except Exception as exc:
            print(f"[Zenoh] Not available ({exc}). Zenoh bots will be disabled.")

    def _on_response(self, sample):
        """Called from Zenoh's internal thread — must not touch asyncio directly."""
        try:
            data = json.loads(bytes(sample.payload).decode("utf-8"))
            request_id = data.get("request_id")
            content = data.get("content", "")
            if request_id and request_id in self._pending:
                future = self._pending.pop(request_id)
                if not future.done():
                    self._loop.call_soon_threadsafe(future.set_result, content)
        except Exception:
            pass

    async def request(
        self,
        room_id: str,
        bot_name: str,
        message: str,
        history: list[dict],
        timeout: float = 60.0,
    ) -> str:
        if not self.available:
            raise RuntimeError("Zenoh router is not reachable. Is it running?")

        request_id = str(uuid.uuid4())
        loop = asyncio.get_event_loop()
        future: asyncio.Future = loop.create_future()
        self._pending[request_id] = future

        payload = json.dumps({
            "request_id": request_id,
            "room_id": room_id,
            "bot_name": bot_name,
            "message": message,
            "history": history,
        })

        await loop.run_in_executor(
            None,
            lambda: self._session.put(f"chat/{room_id}/request/{bot_name}", payload),
        )

        try:
            return await asyncio.wait_for(future, timeout=timeout)
        except asyncio.TimeoutError:
            self._pending.pop(request_id, None)
            raise TimeoutError(
                f"@{bot_name} did not respond within {timeout}s. "
                "Is the bot agent running and connected to the Zenoh router?"
            )

    def close(self):
        if self._session:
            try:
                self._session.close()
            except Exception:
                pass


zenoh_bridge = ZenohBridge()
