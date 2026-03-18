'use client'

import { useState } from 'react'

interface Props {
  room: { id: string; has_bot: number; bot_name: string; bot_provider: string }
  onUpdate: (updated: { has_bot: number; bot_name: string; bot_provider: string }) => void
  onClose: () => void
}

export default function BotSettings({ room, onUpdate, onClose }: Props) {
  const [hasBot, setHasBot] = useState(room.has_bot === 1)
  const [botName, setBotName] = useState(room.bot_name)
  const [botProvider, setBotProvider] = useState<'claude' | 'local'>(
    room.bot_provider === 'local' ? 'local' : 'claude'
  )
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function handleSave() {
    setLoading(true)
    setError(null)

    const res = await fetch(`/api/rooms/${room.id}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ has_bot: hasBot, bot_name: botName, bot_provider: botProvider }),
    })

    if (!res.ok) {
      const data = await res.json()
      setError(data.error ?? 'Failed to save settings')
      setLoading(false)
      return
    }

    onUpdate({ has_bot: hasBot ? 1 : 0, bot_name: botName || 'bot', bot_provider: botProvider })
  }

  return (
    <div className="bg-gray-900 border-b border-gray-800 px-4 py-4 shrink-0">
      <div className="max-w-sm space-y-3">
        <h3 className="text-white font-semibold text-sm">Bot Settings</h3>

        {error && (
          <p className="text-red-400 text-xs bg-red-950/50 border border-red-900/50 px-3 py-2 rounded-lg">
            {error}
          </p>
        )}

        <label className="flex items-center gap-2.5 cursor-pointer">
          <input
            type="checkbox"
            checked={hasBot}
            onChange={e => setHasBot(e.target.checked)}
            className="w-4 h-4 rounded accent-indigo-600"
          />
          <span className="text-gray-300 text-sm">Enable AI bot</span>
        </label>

        {hasBot && (
          <>
            <div>
              <label className="text-gray-400 text-xs block mb-1">Bot name</label>
              <input
                type="text"
                value={botName}
                onChange={e => setBotName(e.target.value)}
                className="w-full bg-gray-800 text-white px-3 py-2 rounded-lg border border-gray-700 focus:outline-none focus:border-indigo-500 text-sm transition-colors"
              />
            </div>

            <div>
              <label className="text-gray-400 text-xs block mb-1.5">AI provider</label>
              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={() => setBotProvider('claude')}
                  className={`flex-1 py-2 px-3 rounded-lg text-sm font-medium border transition-colors ${
                    botProvider === 'claude'
                      ? 'bg-indigo-600 border-indigo-500 text-white'
                      : 'bg-gray-800 border-gray-700 text-gray-400 hover:text-white'
                  }`}
                >
                  Claude (cloud)
                </button>
                <button
                  type="button"
                  onClick={() => setBotProvider('local')}
                  className={`flex-1 py-2 px-3 rounded-lg text-sm font-medium border transition-colors ${
                    botProvider === 'local'
                      ? 'bg-indigo-600 border-indigo-500 text-white'
                      : 'bg-gray-800 border-gray-700 text-gray-400 hover:text-white'
                  }`}
                >
                  Local (Ollama)
                </button>
              </div>
              {botProvider === 'local' && (
                <p className="text-gray-600 text-xs mt-1.5">
                  Uses <code className="text-gray-500">LOCAL_LLM_URL</code> and <code className="text-gray-500">LOCAL_LLM_MODEL</code> from .env.local
                </p>
              )}
            </div>
          </>
        )}

        <div className="flex gap-2">
          <button
            onClick={handleSave}
            disabled={loading}
            className="bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white text-sm font-medium px-4 py-2 rounded-lg transition-colors"
          >
            {loading ? 'Saving…' : 'Save'}
          </button>
          <button
            onClick={onClose}
            className="text-gray-400 hover:text-white text-sm px-3 py-2 rounded-lg transition-colors"
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  )
}
