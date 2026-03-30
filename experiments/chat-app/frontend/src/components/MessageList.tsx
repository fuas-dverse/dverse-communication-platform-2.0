import { useEffect, useRef } from "react"
import type { Message } from "../types"

interface Props {
  messages: Message[]
  currentUserId: string
}

function formatTime(iso: string): string {
  const date = new Date(iso)
  const now = new Date()
  const isToday =
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate()
  const time = date.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })
  return isToday ? `Today at ${time}` : `${date.toLocaleString("default", { month: "short" })} ${date.getDate()} at ${time}`
}

function getDateKey(iso: string): string {
  const d = new Date(iso)
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`
}

function getTodayKey(): string {
  const d = new Date()
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`
}

function dateLabel(iso: string): string {
  const d = new Date(iso)
  const key = getDateKey(iso)
  if (key === getTodayKey()) return "Today"
  return d.toLocaleDateString("default", { month: "long", day: "numeric", year: "numeric" })
}

interface Group {
  key: string
  isBot: boolean
  username: string
  messages: Message[]
}

function buildGroups(messages: Message[]): Group[] {
  const groups: Group[] = []
  for (const msg of messages) {
    // Skip raw "thinking..." — it's shown in the typing indicator
    if (msg.is_bot && msg.content === "thinking...") continue
    const key = msg.is_bot ? `bot-${msg.bot_id ?? msg.username}` : msg.user_id
    const last = groups[groups.length - 1]
    if (last && last.key === key) {
      last.messages.push(msg)
    } else {
      groups.push({ key, isBot: msg.is_bot, username: msg.username, messages: [msg] })
    }
  }
  return groups
}

function hashColor(name: string, palette: Array<{ bg: string; color: string }>) {
  let hash = 0
  for (const ch of name) hash = (hash * 31 + ch.charCodeAt(0)) | 0
  return palette[Math.abs(hash) % palette.length]
}

const BOT_COLORS = [
  { bg: "#163524", color: "#57f2b8" },
  { bg: "#1a1f3a", color: "#7289da" },
  { bg: "#2d1a1a", color: "#e07d7d" },
  { bg: "#2d1a3a", color: "#c07df0" },
  { bg: "#1a2d1a", color: "#7dc87d" },
]

const USER_COLORS = [
  { bg: "#3b2f6e", color: "#a99ef0" },
  { bg: "#1f2d4a", color: "#7da7e0" },
  { bg: "#2d1f3a", color: "#c0a0f0" },
  { bg: "#1a2d1a", color: "#7dc87d" },
  { bg: "#2d2a1a", color: "#e0c07d" },
]

/** Render text with @mention highlighting */
function renderText(content: string): React.ReactNode {
  const parts = content.split(/(@[\w-]+)/g)
  return parts.map((part, i) =>
    /^@[\w-]/.test(part) ? (
      <span
        key={i}
        style={{
          color: "#57f2b8",
          fontWeight: "500",
          cursor: "pointer",
          background: "rgba(87,242,184,0.1)",
          borderRadius: "3px",
          padding: "0 2px",
        }}
      >
        {part}
      </span>
    ) : (
      part
    )
  )
}

function DateDivider({ label }: { label: string }) {
  return (
    <div
      style={{
        textAlign: "center",
        fontSize: "11px",
        color: "#5f6478",
        margin: "12px 0",
        display: "flex",
        alignItems: "center",
        gap: "8px",
      }}
    >
      <div style={{ flex: 1, height: "1px", background: "#2e3240" }} />
      {label}
      <div style={{ flex: 1, height: "1px", background: "#2e3240" }} />
    </div>
  )
}

function AgentCard({ content, agentName, color, bg }: {
  content: string
  agentName: string
  color: string
  bg: string
}) {
  return (
    <div
      style={{
        background: bg,
        borderRadius: "8px",
        padding: "10px 12px",
        margin: "4px 0",
        borderLeft: `3px solid ${color}`,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: "6px", marginBottom: "6px" }}>
        <span style={{ color, fontSize: "12px", fontWeight: "500" }}>{agentName}</span>
        <span
          style={{
            background: bg,
            color,
            borderRadius: "6px",
            fontSize: "10px",
            padding: "2px 7px",
            fontWeight: "500",
          }}
        >
          AI Agent
        </span>
      </div>
      <div
        style={{
          fontSize: "13px",
          color: "#9a9fad",
          lineHeight: "1.6",
          whiteSpace: "pre-wrap",
          wordBreak: "break-word",
        }}
      >
        {renderText(content)}
      </div>
    </div>
  )
}

function BotMessageGroup({ group }: { group: Group }) {
  const { bg, color } = hashColor(group.username, BOT_COLORS)
  const initials = group.username.slice(0, 2).toUpperCase()

  return (
    <div>
      {group.messages.map((msg, idx) => {
        const isFirst = idx === 0
        if (isFirst) {
          return (
            <div
              key={msg.id}
              style={{ padding: "4px 8px", borderRadius: "6px", display: "flex", gap: "12px" }}
              onMouseEnter={(e) => (e.currentTarget.style.background = "#2e3345")}
              onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
            >
              {/* Bot avatar — rounded square */}
              <div
                style={{
                  width: "36px",
                  height: "36px",
                  borderRadius: "10px",
                  background: bg,
                  color,
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  fontSize: "12px",
                  fontWeight: "500",
                  flexShrink: 0,
                  marginTop: "2px",
                }}
              >
                {initials}
              </div>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div
                  style={{ display: "flex", alignItems: "center", gap: "8px", marginBottom: "2px" }}
                >
                  <span style={{ fontWeight: "500", fontSize: "14px", color }}>{msg.username}</span>
                  <span
                    style={{
                      background: "#2dab7a",
                      color: "#fff",
                      borderRadius: "4px",
                      fontSize: "10px",
                      padding: "1px 5px",
                      fontWeight: "500",
                    }}
                  >
                    AI Agent
                  </span>
                  <span style={{ fontSize: "11px", color: "#5f6478" }}>{formatTime(msg.created_at)}</span>
                </div>
                <AgentCard content={msg.content} agentName={msg.username} color={color} bg={bg} />
              </div>
            </div>
          )
        }
        // Continued bot message
        return (
          <div
            key={msg.id}
            style={{ padding: "2px 8px 2px 56px", borderRadius: "6px" }}
            onMouseEnter={(e) => (e.currentTarget.style.background = "#2e3345")}
            onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
          >
            <AgentCard content={msg.content} agentName={msg.username} color={color} bg={bg} />
          </div>
        )
      })}
    </div>
  )
}

function HumanMessageGroup({ group }: { group: Group }) {
  const { bg, color } = hashColor(group.username, USER_COLORS)
  const initials = group.username.slice(0, 2).toUpperCase()

  return (
    <div>
      {group.messages.map((msg, idx) => {
        const isFirst = idx === 0
        if (isFirst) {
          return (
            <div
              key={msg.id}
              style={{ padding: "4px 8px", borderRadius: "6px", display: "flex", gap: "12px" }}
              onMouseEnter={(e) => (e.currentTarget.style.background = "#2e3345")}
              onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
            >
              {/* Human avatar — circle */}
              <div
                style={{
                  width: "36px",
                  height: "36px",
                  borderRadius: "50%",
                  background: bg,
                  color,
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  fontSize: "12px",
                  fontWeight: "500",
                  flexShrink: 0,
                  marginTop: "2px",
                }}
              >
                {initials}
              </div>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div
                  style={{ display: "flex", alignItems: "center", gap: "8px", marginBottom: "2px" }}
                >
                  <span style={{ fontWeight: "500", fontSize: "14px", color }}>{msg.username}</span>
                  <span style={{ fontSize: "11px", color: "#5f6478" }}>{formatTime(msg.created_at)}</span>
                </div>
                <div
                  style={{
                    fontSize: "13px",
                    color: "#9a9fad",
                    lineHeight: "1.55",
                    whiteSpace: "pre-wrap",
                    wordBreak: "break-word",
                  }}
                >
                  {renderText(msg.content)}
                </div>
              </div>
            </div>
          )
        }
        // Continued human message
        return (
          <div
            key={msg.id}
            style={{ padding: "2px 8px 2px 56px", borderRadius: "6px" }}
            onMouseEnter={(e) => (e.currentTarget.style.background = "#2e3345")}
            onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
          >
            <div
              style={{
                fontSize: "13px",
                color: "#9a9fad",
                lineHeight: "1.55",
                whiteSpace: "pre-wrap",
                wordBreak: "break-word",
              }}
            >
              {renderText(msg.content)}
            </div>
          </div>
        )
      })}
    </div>
  )
}

export default function MessageList({ messages }: Props) {
  const bottomRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" })
  }, [messages])

  const groups = buildGroups(messages)

  if (groups.length === 0) {
    return (
      <div
        style={{
          flex: 1,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: "#5f6478",
          fontSize: "13px",
        }}
      >
        No messages yet. Say hello!
      </div>
    )
  }

  // Compute date dividers
  const items = groups.map((group, i) => {
    const dateKey = getDateKey(group.messages[0].created_at)
    const prevGroup = groups[i - 1]
    const prevKey = prevGroup ? getDateKey(prevGroup.messages[0].created_at) : null
    return {
      group,
      showDivider: dateKey !== prevKey,
      dividerLabel: dateLabel(group.messages[0].created_at),
    }
  })

  return (
    <div
      className="scrollbar-thin"
      style={{
        flex: 1,
        overflowY: "auto",
        padding: "16px 16px 0",
        display: "flex",
        flexDirection: "column",
      }}
    >
      {items.map(({ group, showDivider, dividerLabel }) => (
        <div key={`${group.key}-${group.messages[0].id}`}>
          {showDivider && <DateDivider label={dividerLabel} />}
          {group.isBot ? (
            <BotMessageGroup group={group} />
          ) : (
            <HumanMessageGroup group={group} />
          )}
        </div>
      ))}
      <div ref={bottomRef} style={{ height: "16px", flexShrink: 0 }} />
    </div>
  )
}
