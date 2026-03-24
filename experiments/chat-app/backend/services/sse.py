import asyncio
import json
from collections import defaultdict


class SSEBroker:
    def __init__(self):
        self._queues: dict[str, list[asyncio.Queue]] = defaultdict(list)

    async def subscribe(self, room_id: str) -> asyncio.Queue:
        q: asyncio.Queue = asyncio.Queue()
        self._queues[room_id].append(q)
        return q

    def unsubscribe(self, room_id: str, q: asyncio.Queue):
        queues = self._queues.get(room_id, [])
        if q in queues:
            queues.remove(q)

    async def publish(self, room_id: str, data: dict):
        queues = self._queues.get(room_id, [])
        dead = []
        for q in queues:
            try:
                await q.put(data)
            except Exception:
                dead.append(q)
        for q in dead:
            self.unsubscribe(room_id, q)


broker = SSEBroker()
