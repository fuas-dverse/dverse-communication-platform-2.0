import { useState, useEffect, useRef, useCallback } from "react"
import { useParams, useNavigate, Link } from "react-router-dom"
import { useAuth } from "../store/auth"
import { getRoom } from "../api/rooms"
import { getMessages, sendMessage } from "../api/messages"
import { getToken, API_BASE } from "../api/client"
import MessageList from "../components/MessageList"
import MessageInput from "../components/MessageInput"
import type { Room, Message } from "../types"

export default function ChatRoomPage() {
  const { id } = useParams<{ id: string }>()
  const { user } = useAuth()
  const navigate = useNavigate()

  const [room, setRoom] = useState<Room | null>(null)
  const [messages, setMessages] = useState<Message[]>([])
  const [loadingRoom, setLoadingRoom] = useState(true)
  const [loadingMessages, setLoadingMessages] = useState(true)
  const [roomError, setRoomError] = useState<string | null>(null)
  const [sendError, setSendError] = useState<string | null>(null)

  const sseAbortRef = useRef<AbortController | null>(null)

  // Load room
  useEffect(() => {
    if (!id) return
    setLoadingRoom(true)
    getRoom(id)
      .then(setRoom)
      .catch((err) => {
        setRoomError(err instanceof Error ? err.message : "Failed to load room")
      })
      .finally(() => setLoadingRoom(false))
  }, [id])

  // Load initial messages
  useEffect(() => {
    if (!id) return
    setLoadingMessages(true)
    getMessages(id)
      .then(setMessages)
      .catch(() => setMessages([]))
      .finally(() => setLoadingMessages(false))
  }, [id])

  // SSE connection
  const connectSSE = useCallback(async () => {
    if (!id) return

    const token = getToken()
    if (!token) return

    // Abort any existing SSE connection
    sseAbortRef.current?.abort()
    const controller = new AbortController()
    sseAbortRef.current = controller

    try {
      const response = await fetch(`${API_BASE}/rooms/${id}/stream`, {
        headers: { Authorization: `Bearer ${token}` },
        signal: controller.signal,
      })

      if (!response.ok || !response.body) {
        setTimeout(() => { if (!controller.signal.aborted) connectSSE() }, 2000)
        return
      }

      const reader = response.body.getReader()
      const decoder = new TextDecoder()
      let buffer = ""

      while (true) {
        const { done, value } = await reader.read()
        if (done) {
          setTimeout(() => { if (!controller.signal.aborted) connectSSE() }, 1000)
          break
        }

        buffer += decoder.decode(value, { stream: true })

        // SSE messages are separated by double newlines
        const parts = buffer.split("\n\n")
        buffer = parts.pop() ?? ""

        for (const part of parts) {
          const lines = part.split("\n")
          let data: string | null = null

          for (const line of lines) {
            if (line.startsWith("data: ")) {
              data = line.slice(6)
            }
          }

          if (!data) continue

          try {
            const event = JSON.parse(data) as {
              type: "message" | "replace"
              message: Message
            }

            if (event.type === "message") {
              setMessages((prev) => {
                // Avoid duplicates
                if (prev.some((m) => m.id === event.message.id)) return prev
                return [...prev, event.message]
              })
            } else if (event.type === "replace") {
              setMessages((prev) =>
                prev.map((m) =>
                  m.id === event.message.id ? event.message : m
                )
              )
            }
          } catch {
            // Ignore parse errors for individual events
          }
        }
      }
    } catch (err) {
      if (err instanceof Error && err.name === "AbortError") return
      // Reconnect after a short delay on unexpected disconnect
      setTimeout(() => {
        if (!sseAbortRef.current?.signal.aborted) {
          connectSSE()
        }
      }, 3000)
    }
  }, [id])

  useEffect(() => {
    connectSSE()
    return () => {
      sseAbortRef.current?.abort()
    }
  }, [connectSSE])

  async function handleSend(content: string) {
    if (!id) return
    setSendError(null)
    try {
      await sendMessage(id, content)
      // Message will arrive via SSE
    } catch (err) {
      setSendError(err instanceof Error ? err.message : "Failed to send message")
      throw err
    }
  }

  if (loadingRoom) {
    return (
      <div className="flex items-center justify-center min-h-screen bg-gray-900">
        <div className="text-gray-400">Loading room...</div>
      </div>
    )
  }

  if (roomError || !room) {
    return (
      <div className="flex flex-col items-center justify-center min-h-screen bg-gray-900 gap-4">
        <div className="text-red-400">{roomError || "Room not found"}</div>
        <button
          onClick={() => navigate("/rooms")}
          className="text-sm text-indigo-400 hover:text-indigo-300"
        >
          Back to rooms
        </button>
      </div>
    )
  }

  return (
    <div className="flex flex-col h-screen bg-gray-900 items-center">
      <div className="flex flex-col w-full flex-1 min-h-0 border-x border-gray-800">
      {/* Header */}
      <header className="bg-gray-800 border-b border-gray-700 flex-shrink-0">
        <div className="px-4 py-3 flex items-center gap-3">
          <Link
            to="/rooms"
            className="text-gray-400 hover:text-white transition p-1 rounded-lg hover:bg-gray-700"
            aria-label="Back to rooms"
          >
            <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 19l-7-7 7-7" />
            </svg>
          </Link>

          <div className="flex-1 min-w-0">
            <h1 className="font-semibold text-white truncate">{room.name}</h1>
            {room.description && (
              <p className="text-gray-400 text-xs truncate">{room.description}</p>
            )}
          </div>

          <div className="flex items-center gap-2 flex-shrink-0">
            <span className="text-gray-400 text-sm hidden sm:inline-block">
              {user?.username}
            </span>
          </div>
        </div>
      </header>

      {/* Messages */}
      {loadingMessages ? (
        <div className="flex-1 flex items-center justify-center">
          <div className="text-gray-500">Loading messages...</div>
        </div>
      ) : (
        <MessageList
          messages={messages}
          currentUserId={user?.id ?? ""}
        />
      )}

      {/* Send error */}
      {sendError && (
        <div className="px-4 py-2 bg-red-900/40 border-t border-red-700/50 text-red-300 text-sm flex items-center justify-between">
          <span>{sendError}</span>
          <button
            onClick={() => setSendError(null)}
            className="text-red-400 hover:text-red-300 ml-3"
          >
            ×
          </button>
        </div>
      )}

      {/* Input */}
      <MessageInput onSend={handleSend} />
      </div>
    </div>
  )
}
