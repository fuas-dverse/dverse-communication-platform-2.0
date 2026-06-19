"""Unit tests for backend/services/sse.py — SSEBroker."""

import asyncio
import unittest

from backend.services.sse import SSEBroker


class TestSSEBroker(unittest.IsolatedAsyncioTestCase):

    def setUp(self):
        self.broker = SSEBroker()

    async def test_subscribe_creates_queue(self):
        q = await self.broker.subscribe("room-1")
        self.assertIsInstance(q, asyncio.Queue)

    async def test_publish_puts_to_all_queues(self):
        q1 = await self.broker.subscribe("room-1")
        q2 = await self.broker.subscribe("room-1")
        await self.broker.publish("room-1", {"msg": "hello"})
        item1 = await asyncio.wait_for(q1.get(), timeout=1.0)
        item2 = await asyncio.wait_for(q2.get(), timeout=1.0)
        self.assertEqual(item1, {"msg": "hello"})
        self.assertEqual(item2, {"msg": "hello"})

    async def test_unsubscribe_removes_queue(self):
        q = await self.broker.subscribe("room-1")
        self.broker.unsubscribe("room-1", q)
        self.assertNotIn(q, self.broker._queues.get("room-1", []))

    async def test_publish_no_channel_is_noop(self):
        # Should not raise even if no subscribers
        await self.broker.publish("nonexistent-room", {"msg": "hi"})

    async def test_subscribe_multiple_channels(self):
        q1 = await self.broker.subscribe("room-a")
        q2 = await self.broker.subscribe("room-b")
        await self.broker.publish("room-a", {"for": "a"})
        item = await asyncio.wait_for(q1.get(), timeout=1.0)
        self.assertEqual(item, {"for": "a"})
        # room-b queue must be empty
        self.assertTrue(q2.empty())

    async def test_unsubscribe_nonexistent_queue_is_noop(self):
        q = asyncio.Queue()
        # Should not raise
        self.broker.unsubscribe("does-not-exist", q)

    async def test_publish_multiple_times(self):
        q = await self.broker.subscribe("room-1")
        await self.broker.publish("room-1", {"n": 1})
        await self.broker.publish("room-1", {"n": 2})
        item1 = await asyncio.wait_for(q.get(), timeout=1.0)
        item2 = await asyncio.wait_for(q.get(), timeout=1.0)
        self.assertEqual(item1["n"], 1)
        self.assertEqual(item2["n"], 2)


if __name__ == "__main__":
    unittest.main()
