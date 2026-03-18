'use client'

export interface Message {
  id: string
  room_id: string
  user_id: string
  username: string
  content: string
  is_bot: number
  bot_triggered_by: string | null
  created_at: number
}

interface Props {
  messages: Message[]
  currentUserId: string
  botName: string
}

function formatTime(ts: number): string {
  const d = new Date(ts)
  const h = d.getHours().toString().padStart(2, '0')
  const m = d.getMinutes().toString().padStart(2, '0')
  return `${h}:${m}`
}

export default function MessageList({ messages, currentUserId, botName }: Props) {
  if (messages.length === 0) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <p className="text-gray-600 text-sm">No messages yet. Say hello!</p>
      </div>
    )
  }

  return (
    <div className="flex-1 overflow-y-auto px-4 py-4 space-y-1">
      {messages.map((msg, i) => {
        const isOwn = msg.user_id === currentUserId
        const isBot = msg.is_bot === 1
        const showUsername =
          !isOwn &&
          (i === 0 || messages[i - 1].user_id !== msg.user_id)

        return (
          <div
            key={msg.id}
            className={`flex ${isOwn ? 'justify-end' : 'justify-start'} ${i > 0 && messages[i - 1].user_id === msg.user_id ? 'mt-0.5' : 'mt-3'}`}
          >
            <div className={`max-w-[72%] flex flex-col ${isOwn ? 'items-end' : 'items-start'}`}>
              {showUsername && (
                <span className={`text-xs font-medium mb-1 px-1 ${isBot ? 'text-indigo-400' : 'text-gray-400'}`}>
                  {isBot ? `${botName} (AI)` : msg.username}
                </span>
              )}

              <div
                className={`
                  px-4 py-2.5 rounded-2xl text-sm leading-relaxed break-words
                  ${isBot
                    ? 'bg-indigo-950 text-indigo-100 border border-indigo-800/60 rounded-tl-sm'
                    : isOwn
                      ? 'bg-indigo-600 text-white rounded-tr-sm'
                      : 'bg-gray-800 text-gray-100 rounded-tl-sm'
                  }
                `}
              >
                {msg.content}
              </div>

              <div className="flex items-center gap-2 mt-0.5 px-1">
                <span className="text-gray-600 text-xs">{formatTime(msg.created_at)}</span>
                {isBot && msg.bot_triggered_by && (
                  <span className="text-gray-700 text-xs">· triggered by @{msg.bot_triggered_by}</span>
                )}
              </div>
            </div>
          </div>
        )
      })}
    </div>
  )
}
