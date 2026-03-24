import { useState, useEffect, useRef } from "react"
import { useAuth } from "../store/auth"
import { getRooms } from "../api/rooms"
import RoomList from "../components/RoomList"
import type { Room } from "../types"
import { API_BASE, getToken } from "../api/client"

export default function RoomsPage() {
  const { user, logout } = useAuth()
  const [rooms, setRooms] = useState<Room[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const abortRef = useRef<AbortController | null>(null)

  useEffect(() => {
    getRooms()
      .then(setRooms)
      .catch((err) => {
        setError(err instanceof Error ? err.message : "Failed to load rooms")
      })
      .finally(() => setLoading(false))
  }, [])

  useEffect(() => {
    const ctrl = new AbortController()
    abortRef.current = ctrl

    async function connect() {
      try {
        const res = await fetch(`${API_BASE}/rooms/stream`, {
          headers: { Authorization: `Bearer ${getToken()}` },
          signal: ctrl.signal,
        })
        if (!res.ok || !res.body) { setTimeout(connect, 2000); return }
        const reader = res.body.getReader()
        const decoder = new TextDecoder()
        let buf = ""
        while (true) {
          const { done, value } = await reader.read()
          if (done) { setTimeout(connect, 1000); break }
          buf += decoder.decode(value, { stream: true })
          const parts = buf.split("\n\n")
          buf = parts.pop() ?? ""
          for (const part of parts) {
            const dataLine = part.split("\n").find((l) => l.startsWith("data:"))
            if (!dataLine) continue
            try {
              const payload = JSON.parse(dataLine.slice(5).trim())
              if (payload.type === "room_created") {
                setRooms((prev) => [payload.room, ...prev])
              }
            } catch {}
          }
        }
      } catch (e: unknown) {
        if (e instanceof Error && e.name === "AbortError") return
        setTimeout(connect, 3000)
      }
    }

    connect()
    return () => ctrl.abort()
  }, [])

  function handleRoomCreated(_room: Room) {
    // SSE will broadcast the new room to all clients including the creator
  }

  return (
    <div className="min-h-screen bg-gray-900">
      <header className="bg-gray-800 border-b border-gray-700 sticky top-0 z-10">
        <div className="max-w-2xl mx-auto px-4 py-4 flex items-center justify-between">
          <h1 className="text-xl font-bold text-white">ChatApp</h1>
          <div className="flex items-center gap-4">
            <span className="text-gray-300 text-sm">
              {user?.username}
            </span>
            <button
              onClick={logout}
              className="text-sm text-gray-400 hover:text-white bg-gray-700 hover:bg-gray-600 px-3 py-1.5 rounded-lg transition"
            >
              Sign out
            </button>
          </div>
        </div>
      </header>

      <main className="max-w-2xl mx-auto px-4 py-8">
        <div className="mb-6">
          <h2 className="text-2xl font-bold text-white">Rooms</h2>
          <p className="text-gray-400 text-sm mt-1">
            Join a room or create a new one
          </p>
        </div>

        {loading ? (
          <div className="space-y-3">
            {[...Array(3)].map((_, i) => (
              <div
                key={i}
                className="bg-gray-800 rounded-xl p-5 animate-pulse h-20"
              />
            ))}
          </div>
        ) : error ? (
          <div className="bg-red-900/40 border border-red-700 text-red-300 rounded-xl px-4 py-3">
            {error}
          </div>
        ) : (
          <RoomList rooms={rooms} onRoomCreated={handleRoomCreated} />
        )}
      </main>
    </div>
  )
}
