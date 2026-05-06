import { useState } from "react"
import type { Room, Message, BotConfig } from "../types"
import MessageList from "./MessageList"
import MessageInput from "./MessageInput"
import BotSettings from "./BotSettings"
import { Icon } from "@iconify/react"

interface Props {
  room: Room | null
  messages: Message[]
  loadingMessages: boolean
  currentUserId: string
  onSend: (content: string) => Promise<void>
  onBotsChange: (bots: BotConfig[]) => void
  isCreator: boolean
}


export default function ChatArea({
  room,
  messages,
  loadingMessages,
  currentUserId,
  onSend,
  onBotsChange,
  isCreator,
}: Props) {
  const [activeTab, setActiveTab] = useState("Messages")
  const [showBotSettings, setShowBotSettings] = useState(false)

  // Detect if any bot message is currently "thinking"
  const thinkingMsg = [...messages].reverse().find((m) => m.is_bot && m.content === "thinking...")
  const botNames = room?.bots.map((b) => b.name) ?? []

  if (!room) {
    return (
      <div
        style={{
          flex: 1,
          background: "#282d38",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          flexDirection: "column",
          gap: "12px",
        }}
      >
        <div style={{ fontSize: "48px", lineHeight: 1 }}>💬</div>
        <div style={{ color: "#9a9fad", fontSize: "15px", fontWeight: "500" }}>
          Select a channel to start chatting
        </div>
        <div style={{ color: "#5f6478", fontSize: "13px" }}>
          Or create a new channel from the sidebar
        </div>
      </div>
    )
  }

  const memberCount = (room.bots.length) + 1 // bots + current user

  return (
    <div
      style={{
        flex: 1,
        display: "flex",
        flexDirection: "column",
        background: "#282d38",
        minWidth: 0,
      }}
    >
      {/* Header */}
      <div
        style={{
          padding: "10px 16px",
          borderBottom: "1px solid #2e3240",
          display: "flex",
          alignItems: "center",
          gap: "10px",
          flexShrink: 0,
        }}
      >
        <Icon icon="lucide:hash" style={{ fontSize: "18px", color: "#9a9fad", flexShrink: 0 }} />
        <span style={{ fontWeight: "500", fontSize: "15px", color: "#e0e2ea" }}>{room.name}</span>
        {room.description && (
          <span
            style={{
              fontSize: "12px",
              color: "#5f6478",
              paddingLeft: "10px",
              borderLeft: "1px solid #2e3240",
              marginLeft: "4px",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              maxWidth: "300px",
            }}
          >
            {room.description}
          </span>
        )}

        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: "4px" }}>
          <HdrBtn>
            <Icon icon="lucide:users" style={{ fontSize: "14px" }} />{" "}
            <span
              style={{
                background: "#3b3f52",
                borderRadius: "10px",
                padding: "2px 8px",
                fontSize: "11px",
                color: "#9a9fad",
              }}
            >
              {memberCount}
            </span>
          </HdrBtn>
          {isCreator && (
            <HdrBtn
              active={showBotSettings}
              onClick={() => setShowBotSettings((v) => !v)}
            >
              <Icon icon="lucide:settings" style={{ fontSize: "13px" }} /> Bots
            </HdrBtn>
          )}
        </div>
      </div>

      {/* Bot Settings panel (toggled from header) */}
      {showBotSettings && isCreator && (
        <BotSettings roomId={room.id} bots={room.bots} onBotsChange={onBotsChange} />
      )}

      {/* Messages */}
      {loadingMessages ? (
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
          Loading messages…
        </div>
      ) : (
        <MessageList messages={messages} currentUserId={currentUserId} />
      )}

      {/* Typing indicator */}
      {thinkingMsg && (
        <div
          style={{
            padding: "0 16px 10px",
            display: "flex",
            alignItems: "center",
            gap: "8px",
            fontSize: "12px",
            color: "#5f6478",
            flexShrink: 0,
          }}
        >
          <div
            style={{
              width: "20px",
              height: "20px",
              borderRadius: "6px",
              background: "#163524",
              color: "#57f2b8",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              fontSize: "8px",
              fontWeight: "500",
            }}
          >
            {thinkingMsg.username.slice(0, 2).toUpperCase()}
          </div>
          <TypingDots />
          <span>{thinkingMsg.username} is thinking…</span>
        </div>
      )}

      {/* Input */}
      <MessageInput
        onSend={onSend}
        placeholder={
          botNames.length > 0
            ? `Message #${room.name} — or type @agentname to invoke an AI agent`
            : `Message #${room.name}`
        }
        botNames={botNames}
      />
    </div>
  )
}

function HdrBtn({
  children,
  active = false,
  onClick,
}: {
  children: React.ReactNode
  active?: boolean
  onClick?: () => void
}) {
  const [hovered, setHovered] = useState(false)
  return (
    <button
      onClick={onClick}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        padding: "4px 8px",
        borderRadius: "6px",
        border: "none",
        background: active ? "#5865f2" : hovered ? "#2e3345" : "transparent",
        color: active ? "#fff" : hovered ? "#e0e2ea" : "#9a9fad",
        cursor: "pointer",
        fontSize: "12px",
        display: "flex",
        alignItems: "center",
        gap: "4px",
      }}
    >
      {children}
    </button>
  )
}

function TabBtn({
  label,
  active,
  onClick,
}: {
  label: string
  active: boolean
  onClick: () => void
}) {
  const [hovered, setHovered] = useState(false)
  return (
    <div
      onClick={onClick}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        padding: "8px 14px",
        fontSize: "12px",
        color: active || hovered ? "#e0e2ea" : "#9a9fad",
        cursor: "pointer",
        borderBottom: active ? "2px solid #5865f2" : "2px solid transparent",
        marginBottom: "-1px",
        userSelect: "none",
      }}
    >
      {label}
    </div>
  )
}

function TypingDots() {
  return (
    <div style={{ display: "flex", gap: "3px" }}>
      {[0, 200, 400].map((delay, i) => (
        <div
          key={i}
          style={{
            width: "5px",
            height: "5px",
            borderRadius: "50%",
            background: "#5f6478",
            animation: `typingBounce 1.2s ${delay}ms infinite`,
          }}
        />
      ))}
    </div>
  )
}
