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
import time
import uuid
from typing import Optional

from opentelemetry.trace import Status, StatusCode

PRESENCE_TTL = 90  # seconds — bot considered offline if no heartbeat within this window


class BotPresence:
    def __init__(self, name: str, description: str, platform: str, capabilities: list[str], token_hash: str):
        self.name = name
        self.description = description
        self.platform = platform
        self.capabilities = capabilities
        self.token_hash = token_hash
        self.last_seen: float = time.time()

    def is_online(self) -> bool:
        return (time.time() - self.last_seen) < PRESENCE_TTL

    def verify_token(self, token: str) -> bool:
        import hashlib
        return hashlib.sha256(token.encode()).hexdigest() == self.token_hash

    def to_dict(self) -> dict:
        return {
            "name": self.name,
            "description": self.description,
            "platform": self.platform,
            "capabilities": self.capabilities,
            "last_seen": self.last_seen,
            "online": self.is_online(),
            # token_hash is intentionally omitted — never sent to clients
        }


class ZenohBridge:
    def __init__(self):
        self._session = None
        self._sub = None
        self._presence_sub = None
        self._pending: dict[str, asyncio.Future] = {}
        self._presence: dict[str, BotPresence] = {}
        self._loop: Optional[asyncio.AbstractEventLoop] = None
        self.available = False

    def start(self, loop: asyncio.AbstractEventLoop):
        self._loop = loop
        router = os.environ.get("ZENOH_ROUTER", "tcp/localhost:7447")
        try:
            import zenoh  # optional dependency
            conf = zenoh.Config.from_json5(json.dumps({
                "connect": {"endpoints": [router]}
            }))
            self._session = zenoh.open(conf)
            self._sub = self._session.declare_subscriber(
                "chat/response/**",
                self._on_response,
            )
            self._presence_sub = self._session.declare_subscriber(
                "chat/presence/**",
                self._on_presence,
            )
            self.available = True
            print(f"[Zenoh] Connected to router at {router} (ZID: {self._session.zid()})")
        except Exception as exc:
            print(f"[Zenoh] Not available ({exc}). Zenoh bots will be disabled.")

    def _on_presence(self, sample):
        """Called from Zenoh's internal thread when a bot publishes its heartbeat."""
        try:
            data = json.loads(bytes(sample.payload.to_bytes()).decode("utf-8"))
            name = data.get("name", "")
            if not name:
                return
            if name in self._presence:
                self._presence[name].last_seen = time.time()
                self._presence[name].token_hash = data.get("token_hash", "")
            else:
                self._presence[name] = BotPresence(
                    name=name,
                    description=data.get("description", ""),
                    platform=data.get("platform", "zenoh"),
                    capabilities=data.get("capabilities", []),
                    token_hash=data.get("token_hash", ""),
                )
        except Exception as exc:
            print(f"[Zenoh] _on_presence error: {exc}")

    def get_available_bots(self) -> list[dict]:
        """Return bots that have sent a heartbeat recently."""
        return [b.to_dict() for b in self._presence.values() if b.is_online()]

    def verify_bot_token(self, bot_name: str, token: str) -> bool:
        """Return True if token matches the hash the bot published."""
        presence = self._presence.get(bot_name)
        if not presence or not presence.is_online():
            return False
        return presence.verify_token(token)

    def _on_response(self, sample):
        """Called from Zenoh's internal thread — must not touch asyncio directly."""
        try:
            data = json.loads(bytes(sample.payload.to_bytes()).decode("utf-8"))
            request_id = data.get("request_id")
            content = data.get("content", "")
            if request_id and request_id in self._pending:
                future = self._pending.pop(request_id)
                if not future.done():
                    self._loop.call_soon_threadsafe(future.set_result, content)
        except Exception as exc:
            print(f"[Zenoh] _on_response error: {exc}")

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

        from ..telemetry import tracer, zenoh_duration

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
            lambda: self._session.put(f"chat/{room_id}/request/{bot_name}", payload.encode()),
        )

        try:
            return await asyncio.wait_for(future, timeout=timeout)
        except asyncio.TimeoutError:
            self._pending.pop(request_id, None)
            raise TimeoutError(
                f"@{bot_name} did not respond within {timeout}s. "
                "Is the bot agent running and connected to the Zenoh router?"
            )
        with tracer.start_as_current_span("zenoh.bot_request") as span:
            span.set_attribute("zenoh.room_id", room_id)
            span.set_attribute("zenoh.bot_name", bot_name)
            span.set_attribute("zenoh.request_id", request_id)
            span.set_attribute("zenoh.timeout", timeout)
            t0 = time.monotonic()
            try:
                await loop.run_in_executor(
                    None,
                    lambda: self._session.put(f"chat/{room_id}/request/{bot_name}", payload.encode()),
                )
                return await asyncio.wait_for(future, timeout=timeout)
            except asyncio.TimeoutError:
                self._pending.pop(request_id, None)
                span.set_status(Status(StatusCode.ERROR, "timeout"))
                raise TimeoutError(
                    f"@{bot_name} did not respond within {timeout}s. "
                    "Is the bot agent running and connected to the Zenoh router?"
                )
            except Exception as exc:
                span.record_exception(exc)
                span.set_status(Status(StatusCode.ERROR, str(exc)))
                raise
            finally:
                zenoh_duration.record(time.monotonic() - t0, {"zenoh.bot_name": bot_name})

    def close(self):
        if self._session:
            try:
                self._session.close()
            except Exception:
                pass
        self._presence.clear()


zenoh_bridge = ZenohBridge()
