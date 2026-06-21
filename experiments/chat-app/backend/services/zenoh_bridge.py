"""
Zenoh ↔ Chat bridge.

Two modes:
  1. Direct (plain TCP, no encryption): connects zenoh-python directly to a
     plain Zenoh router.  Useful for dev/testing without DVerse mTLS.
  2. Bridge WebSocket: connects to the local Rust bridge bot running inside
     the DVerse Tauri app.  The Rust side handles mTLS + Megolm decryption
     and exposes plaintext JSON over a local WebSocket.  Use this whenever
     the DVerse session uses end-to-end encryption.

Connection string (base64 JSON {host, port, token}) selects mode 2.
ZENOH_ROUTER env-var selects mode 1.
"""

import asyncio
import base64
import json
import os
import time
import uuid
from typing import Optional

from opentelemetry.trace import Status, StatusCode


PRESENCE_TTL = 90  # seconds


class DverseNode:
    def __init__(self, cn: str, name: str, agents: list[str]):
        self.cn = cn
        self.name = name
        self.agents = agents
        self.last_seen: float = time.time()

    def is_online(self) -> bool:
        return (time.time() - self.last_seen) < PRESENCE_TTL

    def to_dict(self) -> dict:
        return {
            "cn": self.cn,
            "name": self.name,
            "agents": self.agents,
            "online": self.is_online(),
            "last_seen": self.last_seen,
        }


class ZenohBridge:
    def __init__(self):
        self._session = None        # zenoh-python session (mode 1)
        self._ws = None             # websocket connection (mode 2)
        self._ws_task = None        # asyncio task for WS read loop
        self._subs: list = []
        self._loop: Optional[asyncio.AbstractEventLoop] = None
        self._nodes: dict[str, DverseNode] = {}
        self._namespace: str = ""
        self._ws_token: str = ""
        self._ws_port: int = 0
        self._ws_host: str = ""
        self.available = False
        self._broker = None         # injected after broker is initialised
        self._pending: dict[str, asyncio.Future] = {}

    # ── Public API ─────────────────────────────────────────────────────────────

    def configure(self, router: str = "", namespace: str = "",
                  connection_string: str = ""):
        """(Re)configure.  Pass either router+namespace or connection_string."""
        self._close_internal()
        if not self._loop:
            return
        if connection_string:
            self._start_ws(connection_string, self._loop)
        elif router:
            self._start_zenoh(router, namespace, self._loop)

    def start(self, loop: asyncio.AbstractEventLoop):
        self._loop = loop
        conn_str = os.environ.get("DVERSE_CONNECTION_STRING", "")
        router = os.environ.get("ZENOH_ROUTER", "")
        namespace = os.environ.get("ZENOH_NAMESPACE", "")
        if conn_str:
            self._start_ws(conn_str, loop)
        elif router:
            self._start_zenoh(router, namespace, loop)

    def publish_message(self, room_id: str, sender: str, content: str):
        """Publish user message so Zenoh agents see it."""
        payload = json.dumps({
            "id": str(uuid.uuid4()),
            "room_id": room_id,
            "sender": sender,
            "content": content,
            "is_agent": False,
            "created_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        })
        if self._ws and self._loop:
            asyncio.run_coroutine_threadsafe(
                self._ws_send(payload), self._loop
            )
        elif self._session:
            topic = f"dverse/rooms/{room_id}/messages"
            try:
                self._session.put(topic, payload.encode())
            except Exception as exc:
                print(f"[Zenoh] publish_message error: {exc}")

    def send_a2a(self, to: str, room_id: str, content: str):
        """Send a message to a Zenoh bot via A2A inbox topic."""
        payload = json.dumps({"type": "a2a", "to": to, "room_id": room_id, "content": content})
        if self._ws and self._loop:
            asyncio.run_coroutine_threadsafe(
                self._ws_send(payload), self._loop
            )

    def get_online_nodes(self) -> list[dict]:
        return [n.to_dict() for n in self._nodes.values() if n.is_online()]

    def get_session_info(self) -> dict:
        return {
            "connected": self.available,
            "namespace": self._namespace,
            "mode": "websocket" if self._ws else ("direct" if self._session else "none"),
        }

    def close(self):
        self._close_internal()
        self._nodes.clear()

    # ── Mode 2: bridge WebSocket ────────────────────────────────────────────────

    def _start_ws(self, connection_string: str, loop):
        try:
            params = json.loads(base64.b64decode(connection_string).decode())
            host = params["host"]
            port = int(params["port"])
            token = params["token"]
        except Exception as exc:
            print(f"[Zenoh] Bad connection string: {exc}")
            return
        host_override = os.environ.get("BRIDGE_HOST", "")
        if host_override and host in ("127.0.0.1", "localhost"):
            host = host_override
        self._ws_host = host
        self._ws_port = port
        self._ws_token = token
        self._loop = loop
        self._ws_task = asyncio.run_coroutine_threadsafe(
            self._ws_run(host, port, token), loop
        )

    async def _ws_run(self, host: str, port: int, token: str):
        try:
            import websockets
        except ImportError:
            print("[Zenoh] 'websockets' package not installed — run: pip install websockets")
            return

        uri = f"ws://{host}:{port}"
        try:
            async with websockets.connect(uri) as ws:
                self._ws = ws
                # Authenticate
                await ws.send(json.dumps({"token": token}))
                resp = json.loads(await ws.recv())
                if resp.get("error"):
                    print(f"[Zenoh] Bridge auth failed: {resp['error']}")
                    self._ws = None
                    return

                self.available = True
                print(f"[Zenoh] Bridge WebSocket connected to {uri}")

                async for raw in ws:
                    try:
                        data = json.loads(raw)
                        if data.get("type") == "a2a_response":
                            self._handle_room_message(data)
                        elif data.get("is_agent") is True:
                            self._handle_room_message(data)
                        elif "cn" in data:
                            self._handle_node_announce(data)
                    except Exception as exc:
                        print(f"[Zenoh] WS message error: {exc}")
        except Exception as exc:
            print(f"[Zenoh] Bridge WebSocket error: {exc}")
        finally:
            self._ws = None
            self.available = False

    async def _ws_send(self, payload: str):
        if self._ws:
            try:
                await self._ws.send(payload)
            except Exception as exc:
                print(f"[Zenoh] WS send error: {exc}")

    # ── Mode 1: direct zenoh-python ────────────────────────────────────────────

    def _start_zenoh(self, router: str, namespace: str, loop):
        try:
            import zenoh
            conf = zenoh.Config.from_json5(json.dumps({
                "mode": "client",
                "connect": {"endpoints": [router]},
                "scouting": {"multicast": {"enabled": False}},
            }))
            self._session = zenoh.open(conf)
            self._namespace = namespace
            self._loop = loop
            self._subs.append(self._session.declare_subscriber(
                "dverse/nodes/announce/**", self._on_node_announce_raw,
            ))
            self._subs.append(self._session.declare_subscriber(
                "dverse/rooms/*/messages", self._on_room_message_raw,
            ))
            self._subs.append(self._session.declare_subscriber(
                "chat/response/**", self._on_response,
            ))
            self.available = True
            print(f"[Zenoh] Direct connected to {router}")
        except Exception as exc:
            print(f"[Zenoh] Not available ({exc})")
            self.available = False

    def _on_node_announce_raw(self, sample):
        try:
            data = json.loads(bytes(sample.payload.to_bytes()).decode())
            self._handle_node_announce(data)
        except Exception as exc:
            print(f"[Zenoh] _on_node_announce error: {exc}")

    def _on_room_message_raw(self, sample):
        try:
            data = json.loads(bytes(sample.payload.to_bytes()).decode())
            if data.get("is_agent") is True:
                self._handle_room_message(data)
        except Exception as exc:
            print(f"[Zenoh] _on_room_message error: {exc}")

    def _on_response(self, sample):
        """Called from Zenoh's internal thread — must not touch asyncio directly."""
        try:
            data = json.loads(bytes(sample.payload.to_bytes()).decode("utf-8"))
            request_id = data.get("request_id")
            content_text = data.get("content", "")
            if request_id and request_id in self._pending:
                future = self._pending.pop(request_id)
                if not future.done():
                    self._loop.call_soon_threadsafe(future.set_result, content_text)
        except Exception as exc:
            print(f"[Zenoh] _on_response error: {exc}")

    # ── Shared handlers ────────────────────────────────────────────────────────

    def _handle_node_announce(self, data: dict):
        cn = data.get("cn", "")
        if not cn:
            return
        agent_name = data.get("agent_name", "")
        # Use CN as display name (strip domain suffix for readability)
        name = cn.split(".")[0] if "." in cn else cn
        if cn in self._nodes:
            self._nodes[cn].last_seen = time.time()
            if agent_name and agent_name not in self._nodes[cn].agents:
                self._nodes[cn].agents.append(agent_name)
        else:
            agents = [agent_name] if agent_name else []
            self._nodes[cn] = DverseNode(cn=cn, name=name, agents=agents)

    def _handle_room_message(self, data: dict):
        if not self._loop or not self._broker:
            return
        room_id = data.get("room_id", "")
        sender = data.get("sender", "zenoh-agent")
        content = data.get("content", "")
        msg_id = data.get("id", str(uuid.uuid4()))
        created_at = data.get("created_at", time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()))

        if not room_id or not content:
            return

        from ..db import get_db
        db = get_db()
        bot_row = db.execute(
            "SELECT id FROM room_bots WHERE room_id = ? AND LOWER(name) = ?",
            (room_id, sender.lower()),
        ).fetchone()
        bot_id = bot_row["id"] if bot_row else None

        placeholder = None
        if bot_id:
            placeholder = db.execute(
                "SELECT id FROM messages WHERE room_id = ? AND bot_id = ? AND content = 'thinking...' ORDER BY created_at DESC LIMIT 1",
                (room_id, bot_id),
            ).fetchone()

        if placeholder:
            db.execute(
                "UPDATE messages SET content = ?, created_at = ? WHERE id = ?",
                (content, created_at, placeholder["id"]),
            )
            db.commit()
            msg_id = placeholder["id"]
            event_type = "replace"
        else:
            db.execute(
                "INSERT OR IGNORE INTO messages (id, room_id, user_id, content, is_bot, bot_id, bot_triggered_by, created_at, bot_hop_count) VALUES (?, ?, 'zenoh', ?, 1, ?, NULL, ?, 0)",
                (msg_id, room_id, content, bot_id, created_at),
            )
            db.commit()
            event_type = "message"

        message = {
            "id": msg_id,
            "room_id": room_id,
            "user_id": "zenoh",
            "username": f"@{sender}",
            "content": content,
            "is_bot": True,
            "bot_id": bot_id,
            "bot_triggered_by": None,
            "created_at": created_at,
        }
        asyncio.run_coroutine_threadsafe(
            self._broker.publish(room_id, {"type": event_type, "message": message}),
            self._loop,
        )

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
        }).encode()

        with tracer.start_as_current_span("zenoh.bot_request") as span:
            span.set_attribute("zenoh.room_id", room_id)
            span.set_attribute("zenoh.bot_name", bot_name)
            span.set_attribute("zenoh.request_id", request_id)
            span.set_attribute("zenoh.timeout", timeout)
            t0 = time.monotonic()
            try:
                await loop.run_in_executor(
                    None,
                    lambda: self._session.put(f"chat/{room_id}/request/{bot_name}", payload),
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

    # ── Cleanup ────────────────────────────────────────────────────────────────

    def _close_internal(self):
        self._subs.clear()
        if self._session:
            try:
                self._session.close()
            except Exception:
                pass
            self._session = None
        if self._ws and self._loop:
            asyncio.run_coroutine_threadsafe(self._ws.close(), self._loop)
        self._ws = None
        if self._ws_task:
            self._ws_task.cancel()
            self._ws_task = None
        self.available = False


zenoh_bridge = ZenohBridge()
