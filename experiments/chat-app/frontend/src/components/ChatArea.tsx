import { useState } from "react"
import type { Room, Message } from "../types"
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
  onBotsChange: (bots: import("../types").BotConfig[]) => void
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
  const [showBotSettings, setShowBotSettings] = useState(false)
  // Detect if any agent message is currently "thinking"
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
        {isCreator && (
          <button
            onClick={() => setShowBotSettings(v => !v)}
            style={{
              marginLeft: "auto",
              padding: "4px 10px",
              borderRadius: "6px",
              border: "none",
              background: showBotSettings ? "#5865f2" : "transparent",
              color: showBotSettings ? "#fff" : "#9a9fad",
              cursor: "pointer",
              fontSize: "12px",
              display: "flex",
              alignItems: "center",
              gap: "4px",
            }}
          >
            <Icon icon="lucide:cpu" style={{ fontSize: "13px" }} /> Agents
          </button>
        )}
      </div>

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
            ? `Message #${room.name} — @mention an agent to trigger it`
            : `Message #${room.name}`
        }
        botNames={botNames}
      />
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
