import { useEffect, useRef } from "react"
import type { Message } from "../types"

interface Props {
  messages: Message[]
  currentUserId: string
}

function formatTimestamp(iso: string): string {
  const date = new Date(iso)
  const now = new Date()
  const isToday =
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate()
  const time = date.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })
  if (isToday) return time
  return `${date.toLocaleString("default", { month: "short" })} ${date.getDate()} ${time}`
}

interface Group {
  key: string
  isOwn: boolean
  isBot: boolean
  username: string
  messages: Message[]
}

function buildGroups(messages: Message[], currentUserId: string): Group[] {
  const groups: Group[] = []
  for (const msg of messages) {
    const isOwn = msg.user_id === currentUserId && !msg.is_bot
    const isBot = msg.is_bot
    const key = isBot ? `bot-${msg.bot_id}` : msg.user_id
    const last = groups[groups.length - 1]
    if (last && last.key === key) {
      last.messages.push(msg)
    } else {
      groups.push({ key, isOwn, isBot, username: msg.username, messages: [msg] })
    }
  }
  return groups
}

function BotGroup({ group }: { group: Group }) {
  return (
    <div className="flex items-start gap-2">
      <div className="flex-shrink-0 w-7 h-7 rounded-full bg-indigo-900/60 border border-indigo-700/50 flex items-center justify-center text-xs mt-0.5">
        🤖
      </div>
      <div className="flex flex-col gap-0.5 min-w-0">
        <span className="text-xs font-semibold text-indigo-400 mb-0.5">
          @{group.username}
        </span>
        {group.messages.map((msg, i) => {
          const isThinking = msg.content === "thinking..."
          return (
            <div key={msg.id} className="flex items-end gap-1.5">
              <div
                className={`bg-gray-800 border border-gray-700 rounded-2xl rounded-tl-sm px-3 py-1.5 text-sm text-gray-200 leading-relaxed max-w-xs ${
                  isThinking ? "animate-pulse" : ""
                }`}
              >
                {isThinking ? (
                  <span className="text-gray-500 italic">thinking...</span>
                ) : (
                  <span className="whitespace-pre-wrap break-words">{msg.content}</span>
                )}
              </div>
              {i === group.messages.length - 1 && (
                <span className="text-xs text-gray-600 pb-0.5 flex-shrink-0">
                  {formatTimestamp(msg.created_at)}
                </span>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}

function OwnGroup({ group }: { group: Group }) {
  return (
    <div className="flex items-start gap-2 flex-row-reverse">
      <div className="flex-shrink-0 w-7 h-7 rounded-full bg-indigo-600 flex items-center justify-center text-xs font-bold text-white uppercase mt-0.5">
        {group.username[0]}
      </div>
      <div className="flex flex-col gap-0.5 items-end min-w-0">
        <span className="text-xs font-semibold text-indigo-400 mb-0.5">You</span>
        {group.messages.map((msg, i) => (
          <div key={msg.id} className="flex items-end gap-1.5 flex-row-reverse">
            <div className="bg-indigo-600 rounded-2xl rounded-tr-sm px-3 py-1.5 text-sm text-white leading-relaxed max-w-xs">
              <span className="whitespace-pre-wrap break-words">{msg.content}</span>
            </div>
            {i === group.messages.length - 1 && (
              <span className="text-xs text-gray-600 pb-0.5 flex-shrink-0">
                {formatTimestamp(msg.created_at)}
              </span>
            )}
          </div>
        ))}
      </div>
    </div>
  )
}

function OtherGroup({ group }: { group: Group }) {
  return (
    <div className="flex items-start gap-2">
      <div className="flex-shrink-0 w-7 h-7 rounded-full bg-gray-600 flex items-center justify-center text-xs font-bold text-white uppercase mt-0.5">
        {group.username[0]}
      </div>
      <div className="flex flex-col gap-0.5 min-w-0">
        <span className="text-xs font-semibold text-gray-400 mb-0.5">{group.username}</span>
        {group.messages.map((msg, i) => (
          <div key={msg.id} className="flex items-end gap-1.5">
            <div className="bg-gray-800 border border-gray-700 rounded-2xl rounded-tl-sm px-3 py-1.5 text-sm text-gray-200 leading-relaxed max-w-xs">
              <span className="whitespace-pre-wrap break-words">{msg.content}</span>
            </div>
            {i === group.messages.length - 1 && (
              <span className="text-xs text-gray-600 pb-0.5 flex-shrink-0">
                {formatTimestamp(msg.created_at)}
              </span>
            )}
          </div>
        ))}
      </div>
    </div>
  )
}

export default function MessageList({ messages, currentUserId }: Props) {
  const bottomRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" })
  }, [messages])

  if (messages.length === 0) {
    return (
      <div className="flex-1 flex items-center justify-center text-gray-600 text-sm">
        No messages yet
      </div>
    )
  }

  const groups = buildGroups(messages, currentUserId)

  return (
    <div className="flex-1 overflow-y-auto px-4 py-3 space-y-3 scrollbar-thin">
      {groups.map((group) =>
        group.isBot ? (
          <BotGroup key={`${group.key}-${group.messages[0].id}`} group={group} />
        ) : group.isOwn ? (
          <OwnGroup key={`${group.key}-${group.messages[0].id}`} group={group} />
        ) : (
          <OtherGroup key={`${group.key}-${group.messages[0].id}`} group={group} />
        )
      )}
      <div ref={bottomRef} />
    </div>
  )
}
