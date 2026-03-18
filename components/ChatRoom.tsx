'use client'

import { useState, useEffect, useRef } from 'react'
import Link from 'next/link'
import MessageList, { type Message } from './MessageList'
import MessageInput from './MessageInput'
import BotSettings from './BotSettings'

interface Room {
  id: string
  name: string
  description: string
  created_by: string
  has_bot: number
  bot_name: string
  bot_provider: string
}

interface Props {
  room: Room
  currentUser: { id: string; username: string }
  initialMessages: Message[]
}

export default function ChatRoom({ room: initialRoom, currentUser, initialMessages }: Props) {
  const [room, setRoom] = useState(initialRoom)
  const [messages, setMessages] = useState<Message[]>(initialMessages)
  const [isSending, setIsSending] = useState(false)
  const [showBotSettings, setShowBotSettings] = useState(false)
  const bottomRef = useRef<HTMLDivElement>(null)
  const isCreator = currentUser.id === room.created_by

  // Auto-scroll on new messages
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages])

  // SSE — server pushes messages instantly
  useEffect(() => {
    const es = new EventSource(`/api/rooms/${room.id}/stream`)

    es.onmessage = (e) => {
      if (e.data === 'ping') return
      const msg: Message & { replace?: boolean } = JSON.parse(e.data)
      setMessages(prev => {
        if (msg.replace) {
          // Update existing message in-place (bot placeholder → real reply)
          return prev.map(m => m.id === msg.id ? msg : m)
        }
        if (prev.some(m => m.id === msg.id)) return prev
        return [...prev, msg]
      })
    }

    es.onerror = () => {
      // Browser will auto-reconnect; nothing to do here
    }

    return () => es.close()
  }, [room.id])

  async function handleSend(content: string) {
    setIsSending(true)
    try {
      await fetch(`/api/rooms/${room.id}/messages`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ content }),
      })
    } finally {
      setIsSending(false)
    }
  }

  return (
    <div className="flex flex-col h-screen bg-gray-950">
      {/* Header */}
      <header className="bg-gray-900 border-b border-gray-800 px-4 py-3 flex items-center justify-between shrink-0">
        <div className="flex items-center gap-3 min-w-0">
          <Link
            href="/rooms"
            className="text-gray-400 hover:text-white transition-colors shrink-0 text-lg leading-none"
          >
            ←
          </Link>
          <div className="min-w-0">
            <h1 className="text-white font-bold truncate"># {room.name}</h1>
            {room.description && (
              <p className="text-gray-500 text-xs truncate">{room.description}</p>
            )}
          </div>
        </div>

        <div className="flex items-center gap-3 shrink-0">
          {room.has_bot === 1 && (
            <span className="hidden sm:flex text-xs bg-indigo-950 text-indigo-400 border border-indigo-800/50 px-2.5 py-1 rounded-full items-center gap-1">
              <span className="w-1.5 h-1.5 bg-indigo-400 rounded-full" />
              AI @{room.bot_name}
            </span>
          )}
          {isCreator && (
            <button
              onClick={() => setShowBotSettings(v => !v)}
              className="text-gray-400 hover:text-white transition-colors text-sm px-3 py-1.5 rounded-lg hover:bg-gray-800"
            >
              ⚙ Settings
            </button>
          )}
          <span className="text-gray-500 text-xs hidden sm:block">@{currentUser.username}</span>
        </div>
      </header>

      {/* Bot settings panel */}
      {showBotSettings && isCreator && (
        <BotSettings
          room={room}
          onUpdate={updated => {
            setRoom(r => ({ ...r, ...updated }))
            setShowBotSettings(false)
          }}
          onClose={() => setShowBotSettings(false)}
        />
      )}

      {/* Messages */}
      <MessageList messages={messages} currentUserId={currentUser.id} botName={room.bot_name} />
      <div ref={bottomRef} />

      {/* Input */}
      <MessageInput
        onSend={handleSend}
        isSending={isSending}
        hasBotHint={room.has_bot === 1}
        botName={room.bot_name}
      />
    </div>
  )
}
