import { useState, type FormEvent } from "react"
import { Link } from "react-router-dom"
import { createRoom } from "../api/rooms"
import type { Room } from "../types"

interface Props {
  rooms: Room[]
  onRoomCreated: (room: Room) => void
}

export default function RoomList({ rooms, onRoomCreated }: Props) {
  const [name, setName] = useState("")
  const [description, setDescription] = useState("")
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [showForm, setShowForm] = useState(false)

  async function handleCreate(e: FormEvent) {
    e.preventDefault()
    setError(null)
    setLoading(true)
    try {
      const room = await createRoom({ name, description })
      onRoomCreated(room)
      setName("")
      setDescription("")
      setShowForm(false)
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create room")
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="space-y-4">
      {rooms.length === 0 ? (
        <div className="text-center py-12 text-gray-500">
          <p className="text-lg">No rooms yet.</p>
          <p className="text-sm mt-1">Create one to get started.</p>
        </div>
      ) : (
        <div className="space-y-3">
          {rooms.map((room) => (
            <Link
              key={room.id}
              to={`/rooms/${room.id}`}
              className="block bg-gray-800 hover:bg-gray-750 border border-gray-700 hover:border-indigo-500/50 rounded-xl p-5 transition group"
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <h3 className="font-semibold text-white group-hover:text-indigo-300 transition truncate">
                    {room.name}
                  </h3>
                  {room.description && (
                    <p className="text-gray-400 text-sm mt-1 line-clamp-2">
                      {room.description}
                    </p>
                  )}
                </div>
                <div className="flex-shrink-0 flex items-center gap-2">
                  <svg
                    className="w-4 h-4 text-gray-500 group-hover:text-indigo-400 transition"
                    fill="none"
                    viewBox="0 0 24 24"
                    stroke="currentColor"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M9 5l7 7-7 7"
                    />
                  </svg>
                </div>
              </div>
            </Link>
          ))}
        </div>
      )}

      {showForm ? (
        <div className="bg-gray-800 border border-gray-700 rounded-xl p-5">
          <h3 className="font-semibold text-white mb-4">New Room</h3>
          <form onSubmit={handleCreate} className="space-y-4">
            <div>
              <label className="block text-sm font-medium text-gray-300 mb-1.5">
                Room Name
              </label>
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                required
                className="w-full bg-gray-700 border border-gray-600 text-white rounded-lg px-4 py-2.5 focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:border-transparent placeholder-gray-500 transition"
                placeholder="e.g. General"
              />
            </div>
            <div>
              <label className="block text-sm font-medium text-gray-300 mb-1.5">
                Description{" "}
                <span className="text-gray-500 font-normal">(optional)</span>
              </label>
              <input
                type="text"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                className="w-full bg-gray-700 border border-gray-600 text-white rounded-lg px-4 py-2.5 focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:border-transparent placeholder-gray-500 transition"
                placeholder="What is this room about?"
              />
            </div>

            {error && (
              <div className="bg-red-900/40 border border-red-700 text-red-300 rounded-lg px-4 py-3 text-sm">
                {error}
              </div>
            )}

            <div className="flex gap-3">
              <button
                type="submit"
                disabled={loading}
                className="flex-1 bg-indigo-600 hover:bg-indigo-500 disabled:bg-indigo-800 disabled:cursor-not-allowed text-white font-semibold rounded-lg px-4 py-2.5 transition"
              >
                {loading ? "Creating..." : "Create Room"}
              </button>
              <button
                type="button"
                onClick={() => {
                  setShowForm(false)
                  setError(null)
                  setName("")
                  setDescription("")
                }}
                className="px-4 py-2.5 bg-gray-700 hover:bg-gray-600 text-gray-300 font-semibold rounded-lg transition"
              >
                Cancel
              </button>
            </div>
          </form>
        </div>
      ) : (
        <button
          onClick={() => setShowForm(true)}
          className="w-full border-2 border-dashed border-gray-700 hover:border-indigo-500/50 text-gray-500 hover:text-indigo-400 rounded-xl py-4 transition flex items-center justify-center gap-2 group"
        >
          <svg
            className="w-5 h-5"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M12 4v16m8-8H4"
            />
          </svg>
          <span className="font-medium">New Room</span>
        </button>
      )}
    </div>
  )
}
