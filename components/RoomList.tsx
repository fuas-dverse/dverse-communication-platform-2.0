'use client'

import { useState } from 'react'
import { useRouter } from 'next/navigation'
import Link from 'next/link'

interface Room {
  id: string
  name: string
  description: string
  created_by: string
  has_bot: number
  bot_name: string
  created_at: number
}

interface Props {
  initialRooms: Room[]
  currentUser: { id: string; username: string }
}

export default function RoomList({ initialRooms, currentUser }: Props) {
  const [rooms, setRooms] = useState<Room[]>(initialRooms)
  const [showCreate, setShowCreate] = useState(false)
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [hasBot, setHasBot] = useState(false)
  const [botName, setBotName] = useState('bot')
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const router = useRouter()

  async function handleCreate(e: React.FormEvent) {
    e.preventDefault()
    setLoading(true)
    setError(null)

    const res = await fetch('/api/rooms', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name, description, has_bot: hasBot, bot_name: botName }),
    })

    const data = await res.json()
    if (!res.ok) {
      setError(data.error ?? 'Failed to create room')
      setLoading(false)
      return
    }

    setRooms(prev => [data.room, ...prev])
    setName('')
    setDescription('')
    setHasBot(false)
    setBotName('bot')
    setShowCreate(false)
    setLoading(false)
    router.push(`/rooms/${data.room.id}`)
  }

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <h2 className="text-white text-xl font-semibold">Rooms</h2>
        <button
          onClick={() => setShowCreate(v => !v)}
          className="bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-medium px-4 py-2 rounded-lg transition-colors"
        >
          {showCreate ? 'Cancel' : '+ New Room'}
        </button>
      </div>

      {showCreate && (
        <form
          onSubmit={handleCreate}
          className="bg-gray-900 border border-gray-800 rounded-xl p-5 mb-6 space-y-4"
        >
          <h3 className="text-white font-semibold">Create a new room</h3>

          {error && (
            <p className="text-red-400 text-sm bg-red-950/50 border border-red-900/50 px-3 py-2 rounded-lg">
              {error}
            </p>
          )}

          <div>
            <label className="text-gray-400 text-sm block mb-1.5">Room name *</label>
            <input
              type="text"
              value={name}
              onChange={e => setName(e.target.value)}
              placeholder="e.g. general, tech-talk"
              className="w-full bg-gray-800 text-white px-4 py-2.5 rounded-lg border border-gray-700 focus:outline-none focus:border-indigo-500 transition-colors placeholder:text-gray-600"
              required
              autoFocus
            />
          </div>

          <div>
            <label className="text-gray-400 text-sm block mb-1.5">Description</label>
            <input
              type="text"
              value={description}
              onChange={e => setDescription(e.target.value)}
              placeholder="What is this room about?"
              className="w-full bg-gray-800 text-white px-4 py-2.5 rounded-lg border border-gray-700 focus:outline-none focus:border-indigo-500 transition-colors placeholder:text-gray-600"
            />
          </div>

          <div className="flex items-center gap-3">
            <label className="flex items-center gap-2.5 cursor-pointer">
              <input
                type="checkbox"
                checked={hasBot}
                onChange={e => setHasBot(e.target.checked)}
                className="w-4 h-4 rounded accent-indigo-600"
              />
              <span className="text-gray-300 text-sm">Enable AI bot</span>
            </label>
          </div>

          {hasBot && (
            <div>
              <label className="text-gray-400 text-sm block mb-1.5">Bot name</label>
              <input
                type="text"
                value={botName}
                onChange={e => setBotName(e.target.value)}
                placeholder="bot"
                className="w-full bg-gray-800 text-white px-4 py-2.5 rounded-lg border border-gray-700 focus:outline-none focus:border-indigo-500 transition-colors"
              />
              <p className="text-gray-600 text-xs mt-1.5">
                Users can trigger the bot by typing @{botName || 'bot'} in the chat
              </p>
            </div>
          )}

          <div className="flex gap-3 pt-1">
            <button
              type="submit"
              disabled={loading}
              className="bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white font-medium px-5 py-2.5 rounded-lg transition-colors text-sm"
            >
              {loading ? 'Creating…' : 'Create Room'}
            </button>
            <button
              type="button"
              onClick={() => setShowCreate(false)}
              className="text-gray-400 hover:text-white px-4 py-2.5 rounded-lg transition-colors text-sm"
            >
              Cancel
            </button>
          </div>
        </form>
      )}

      {rooms.length === 0 ? (
        <div className="text-center py-16">
          <p className="text-gray-500 text-lg">No rooms yet.</p>
          <p className="text-gray-600 text-sm mt-1">Be the first to create one!</p>
        </div>
      ) : (
        <div className="space-y-3">
          {rooms.map(room => (
            <Link
              key={room.id}
              href={`/rooms/${room.id}`}
              className="block bg-gray-900 rounded-xl border border-gray-800 p-4 hover:border-indigo-500/50 transition-colors group"
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <h3 className="text-white font-semibold group-hover:text-indigo-400 transition-colors truncate">
                    # {room.name}
                  </h3>
                  {room.description && (
                    <p className="text-gray-400 text-sm mt-0.5 truncate">{room.description}</p>
                  )}
                  <p className="text-gray-600 text-xs mt-1.5">
                    Created by{' '}
                    <span className="text-gray-500">
                      {room.created_by === currentUser.id ? 'you' : 'someone'}
                    </span>
                  </p>
                </div>
                {room.has_bot === 1 && (
                  <span className="shrink-0 text-xs bg-indigo-950 text-indigo-400 px-2.5 py-1 rounded-full border border-indigo-800/50">
                    AI @{room.bot_name}
                  </span>
                )}
              </div>
            </Link>
          ))}
        </div>
      )}
    </div>
  )
}
