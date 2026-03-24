import { useState, type FormEvent } from "react"
import { addBot, deleteBot } from "../api/rooms"
import type { BotConfig, BotConfigCreate, BotPersonality, BotProvider } from "../types"

interface Props {
  roomId: string
  bots: BotConfig[]
  onBotsChange: (bots: BotConfig[]) => void
}

const PROVIDERS: BotProvider[] = ["claude", "local", "zenoh"]
const PERSONALITIES: BotPersonality[] = ["assistant", "coder", "creative", "analyst"]

const personalityColors: Record<BotPersonality, string> = {
  assistant: "bg-blue-900/50 text-blue-300 border-blue-700/50",
  coder: "bg-green-900/50 text-green-300 border-green-700/50",
  creative: "bg-purple-900/50 text-purple-300 border-purple-700/50",
  analyst: "bg-amber-900/50 text-amber-300 border-amber-700/50",
}

const providerColors: Record<BotProvider, string> = {
  claude: "bg-indigo-900/50 text-indigo-300 border-indigo-700/50",
  local: "bg-gray-700/50 text-gray-300 border-gray-600/50",
  zenoh: "bg-teal-900/50 text-teal-300 border-teal-700/50",
}

interface AddBotFormData {
  name: string
  provider: BotProvider
  personality: BotPersonality
  model: string
}

const DEFAULT_FORM: AddBotFormData = {
  name: "",
  provider: "claude",
  personality: "assistant",
  model: "",
}

export default function BotSettings({ roomId, bots, onBotsChange }: Props) {
  const [showAddForm, setShowAddForm] = useState(false)
  const [form, setForm] = useState<AddBotFormData>(DEFAULT_FORM)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [deletingId, setDeletingId] = useState<string | null>(null)

  function updateForm(field: keyof AddBotFormData, value: string) {
    setForm((prev) => ({ ...prev, [field]: value }))
  }

  async function handleAdd(e: FormEvent) {
    e.preventDefault()
    setError(null)

    if (!/^[a-zA-Z0-9-]+$/.test(form.name)) {
      setError("Name must contain only letters, numbers, and dashes")
      return
    }

    setLoading(true)
    try {
      const payload: BotConfigCreate = {
        name: form.name,
        provider: form.provider,
        personality: form.personality,
        model: form.model.trim() || null,
      }
      const newBot = await addBot(roomId, payload)
      onBotsChange([...bots, newBot])
      setForm(DEFAULT_FORM)
      setShowAddForm(false)
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to add bot")
    } finally {
      setLoading(false)
    }
  }

  async function handleDelete(botId: string) {
    setDeletingId(botId)
    try {
      await deleteBot(roomId, botId)
      onBotsChange(bots.filter((b) => b.id !== botId))
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to delete bot")
    } finally {
      setDeletingId(null)
    }
  }

  return (
    <div className="bg-gray-800/50 border-b border-gray-700">
      <div className="px-4 py-3">
        <h3 className="text-sm font-semibold text-gray-300 mb-3">Bots in this room</h3>

        {bots.length === 0 && !showAddForm && (
          <p className="text-xs text-gray-500 mb-3">No bots configured. Add one to enable AI responses.</p>
        )}

        {bots.length > 0 && (
          <div className="space-y-2 mb-3">
            {bots.map((bot) => (
              <div
                key={bot.id}
                className="flex items-center justify-between bg-gray-800 border border-gray-700 rounded-lg px-3 py-2.5 gap-3"
              >
                <div className="flex items-center gap-2.5 min-w-0">
                  <span className="text-gray-300 font-mono text-sm font-medium truncate">@{bot.name}</span>
                  <span className={`text-xs px-2 py-0.5 rounded-full border ${providerColors[bot.provider]}`}>
                    {bot.provider}
                  </span>
                  <span className={`text-xs px-2 py-0.5 rounded-full border ${personalityColors[bot.personality]}`}>
                    {bot.personality}
                  </span>
                </div>
                <button
                  onClick={() => handleDelete(bot.id)}
                  disabled={deletingId === bot.id}
                  className="flex-shrink-0 text-gray-500 hover:text-red-400 disabled:opacity-50 transition p-1 rounded"
                >
                  {deletingId === bot.id ? (
                    <svg className="w-4 h-4 animate-spin" fill="none" viewBox="0 0 24 24">
                      <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                      <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                    </svg>
                  ) : (
                    <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                    </svg>
                  )}
                </button>
              </div>
            ))}
          </div>
        )}

        {error && (
          <div className="bg-red-900/40 border border-red-700 text-red-300 rounded-lg px-3 py-2 text-xs mb-3">
            {error}
          </div>
        )}

        {showAddForm ? (
          <form onSubmit={handleAdd} className="space-y-3 bg-gray-800 border border-gray-700 rounded-lg p-4">
            <h4 className="text-sm font-medium text-gray-300">Add Bot</h4>

            <div className="grid grid-cols-2 gap-3">
              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1">Name</label>
                <input
                  type="text"
                  value={form.name}
                  onChange={(e) => updateForm("name", e.target.value)}
                  required
                  pattern="[a-zA-Z0-9-]+"
                  className="w-full bg-gray-700 border border-gray-600 text-white rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-1 focus:ring-indigo-500 placeholder-gray-500"
                  placeholder="my-bot"
                />
              </div>
              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1">Provider</label>
                <select
                  value={form.provider}
                  onChange={(e) => updateForm("provider", e.target.value)}
                  className="w-full bg-gray-700 border border-gray-600 text-white rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-1 focus:ring-indigo-500"
                >
                  {PROVIDERS.map((p) => <option key={p} value={p}>{p}</option>)}
                </select>
              </div>
            </div>

            <div className="grid grid-cols-2 gap-3">
              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1">Personality</label>
                <select
                  value={form.personality}
                  onChange={(e) => updateForm("personality", e.target.value)}
                  className="w-full bg-gray-700 border border-gray-600 text-white rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-1 focus:ring-indigo-500"
                >
                  {PERSONALITIES.map((p) => <option key={p} value={p}>{p}</option>)}
                </select>
              </div>
              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1">
                  Model <span className="text-gray-500 font-normal">(optional)</span>
                </label>
                <input
                  type="text"
                  value={form.model}
                  onChange={(e) => updateForm("model", e.target.value)}
                  className="w-full bg-gray-700 border border-gray-600 text-white rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-1 focus:ring-indigo-500 placeholder-gray-500"
                  placeholder="e.g. claude-3-5-sonnet"
                />
              </div>
            </div>

            <div className="flex gap-2">
              <button
                type="submit"
                disabled={loading}
                className="flex-1 bg-indigo-600 hover:bg-indigo-500 disabled:bg-indigo-800 disabled:cursor-not-allowed text-white text-sm font-semibold rounded-lg px-3 py-2 transition"
              >
                {loading ? "Adding..." : "Add Bot"}
              </button>
              <button
                type="button"
                onClick={() => { setShowAddForm(false); setError(null); setForm(DEFAULT_FORM) }}
                className="px-3 py-2 bg-gray-700 hover:bg-gray-600 text-gray-300 text-sm font-semibold rounded-lg transition"
              >
                Cancel
              </button>
            </div>
          </form>
        ) : (
          <button
            onClick={() => setShowAddForm(true)}
            className="w-full text-xs text-gray-500 hover:text-indigo-400 border border-dashed border-gray-700 hover:border-indigo-500/50 rounded-lg py-2 transition flex items-center justify-center gap-1.5"
          >
            <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 4v16m8-8H4" />
            </svg>
            Add Bot
          </button>
        )}
      </div>
    </div>
  )
}
