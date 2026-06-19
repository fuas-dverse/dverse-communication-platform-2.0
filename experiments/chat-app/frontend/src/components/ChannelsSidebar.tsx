import { useState, type CSSProperties, type FormEvent } from "react"
import { createRoom, addBot, deleteRoom } from "../api/rooms"
import type { Room, User, BotProvider, BotPersonality, Server } from "../types"
import { Icon } from "@iconify/react"
import ServerSettings from "./ServerSettings"

interface Props {
  rooms: Room[]
  activeRoomId: string | null
  activeServerId: string | null
  activeServer: Server | null
  user: User | null
  onSelectRoom: (id: string) => void
  onRoomCreated: (room: Room) => void
  onRoomDeleted: (roomId: string) => void
  onLogout: () => void
  loading: boolean
}

const PROVIDERS: BotProvider[] = ["claude", "local", "zenoh"]
const PERSONALITIES: BotPersonality[] = ["assistant", "coder", "creative", "analyst"]

export default function ChannelsSidebar({
  rooms, activeRoomId, activeServerId, activeServer, user,
  onSelectRoom, onRoomCreated, onRoomDeleted, onLogout, loading,
}: Props) {
  const [showCreateForm, setShowCreateForm] = useState(false)
  const [showServerSettings, setShowServerSettings] = useState(false)
  const [channelType, setChannelType] = useState<"text" | "agent">("text")
  const [name, setName] = useState("")
  const [description, setDescription] = useState("")
  const [botName, setBotName] = useState("")
  const [botProvider, setBotProvider] = useState<BotProvider>("claude")
  const [botPersonality, setBotPersonality] = useState<BotPersonality>("assistant")
  const [creating, setCreating] = useState(false)
  const [createError, setCreateError] = useState<string | null>(null)

  const textChannels = rooms.filter((r) => r.bots.length === 0)
  const agentRooms = rooms.filter((r) => r.bots.length > 0)

  function resetForm() {
    setName(""); setDescription(""); setBotName("")
    setBotProvider("claude"); setBotPersonality("assistant")
    setChannelType("text"); setCreateError(null)
  }

  async function handleCreate(e: FormEvent) {
    e.preventDefault()
    if (!activeServerId) return
    setCreateError(null)
    setCreating(true)
    try {
      const room = await createRoom({ name, description, server_id: activeServerId })
      if (channelType === "agent" && botName.trim()) {
        await addBot(room.id, {
          name: botName.trim(),
          provider: botProvider,
          personality: botPersonality,
        })
        // Reload room to get bots attached
        room.bots = [{ id: "", room_id: room.id, name: botName.trim(), provider: botProvider, personality: botPersonality, model: null, created_at: "", system_prompt: null,
            added_by: null }]
      }
      onRoomCreated(room)
      resetForm()
      setShowCreateForm(false)
    } catch (err) {
      setCreateError(err instanceof Error ? err.message : "Failed to create channel")
    } finally {
      setCreating(false)
    }
  }

  return (
    <div
      style={{
        width: "220px",
        background: "#21252e",
        display: "flex",
        flexDirection: "column",
        borderRight: "1px solid #2e3240",
        flexShrink: 0,
      }}
    >
      {/* Header */}
      <div
        style={{
          padding: "14px 14px 10px",
          borderBottom: "1px solid #2e3240",
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
        }}
      >
        <span style={{ fontWeight: "500", fontSize: "14px", color: "#e0e2ea" }}>
          {activeServerId ? "Channels" : "ChatApp"}
        </span>
        <div style={{ display: "flex", gap: "4px" }}>
          {activeServerId && activeServer && user?.id === activeServer.created_by && (
            <button
              title="Server Settings"
              onClick={() => setShowServerSettings(true)}
              style={iconBtnStyle}
              onMouseEnter={(e) => { e.currentTarget.style.background = "#2e3345"; e.currentTarget.style.color = "#e0e2ea" }}
              onMouseLeave={(e) => { e.currentTarget.style.background = "transparent"; e.currentTarget.style.color = "#9a9fad" }}
            >
              <Icon icon="lucide:settings" style={{ fontSize: "14px" }} />
            </button>
          )}
          {activeServerId && (
            <button
              title="New Channel"
              onClick={() => setShowCreateForm((v) => !v)}
              style={iconBtnStyle}
              onMouseEnter={(e) => { e.currentTarget.style.background = "#2e3345"; e.currentTarget.style.color = "#e0e2ea" }}
              onMouseLeave={(e) => { e.currentTarget.style.background = "transparent"; e.currentTarget.style.color = "#9a9fad" }}
            >
              <Icon icon="lucide:plus" style={{ fontSize: "14px" }} />
            </button>
          )}
        </div>
      </div>

      {/* Create channel form */}
      {showCreateForm && activeServerId && (
        <div style={{ padding: "10px", borderBottom: "1px solid #2e3240" }}>
          {/* Type toggle */}
          <div style={{ display: "flex", gap: "4px", marginBottom: "8px" }}>
            {(["text", "agent"] as const).map((t) => (
              <button
                key={t}
                type="button"
                onClick={() => setChannelType(t)}
                style={{
                  flex: 1,
                  padding: "4px 0",
                  borderRadius: "6px",
                  border: "none",
                  background: channelType === t ? "#5865f2" : "#2e3345",
                  color: channelType === t ? "#fff" : "#9a9fad",
                  fontSize: "11px",
                  cursor: "pointer",
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  gap: "4px",
                }}
              >
                {t === "text"
                  ? <><Icon icon="lucide:hash" style={{ fontSize: "11px" }} /> Text</>
                  : <><Icon icon="lucide:cpu" style={{ fontSize: "11px" }} /> Agent</>
                }
              </button>
            ))}
          </div>

          <form onSubmit={handleCreate} style={{ display: "flex", flexDirection: "column", gap: "6px" }}>
            <input
              type="text" value={name} onChange={(e) => setName(e.target.value)}
              required placeholder="Channel name" autoFocus style={inputStyle}
            />
            <input
              type="text" value={description} onChange={(e) => setDescription(e.target.value)}
              placeholder="Description (optional)" style={inputStyle}
            />

            {channelType === "agent" && (
              <>
                <div style={{ height: "1px", background: "#2e3240", margin: "2px 0" }} />
                <div style={{ fontSize: "10px", color: "#5f6478", letterSpacing: "0.05em", textTransform: "uppercase" }}>
                  Agent Config
                </div>
                <input
                  type="text" value={botName} onChange={(e) => setBotName(e.target.value)}
                  required={channelType === "agent"} placeholder="Agent name (e.g. helper-bot)"
                  pattern="[a-zA-Z0-9-]+" style={inputStyle}
                />
                <div style={{ display: "flex", gap: "4px" }}>
                  <select value={botProvider} onChange={(e) => setBotProvider(e.target.value as BotProvider)}
                    style={{ ...inputStyle, flex: 1 }}>
                    {PROVIDERS.map((p) => <option key={p} value={p}>{p}</option>)}
                  </select>
                  <select value={botPersonality} onChange={(e) => setBotPersonality(e.target.value as BotPersonality)}
                    style={{ ...inputStyle, flex: 1 }}>
                    {PERSONALITIES.map((p) => <option key={p} value={p}>{p}</option>)}
                  </select>
                </div>
              </>
            )}

            {createError && (
              <div style={{ color: "#ed4245", fontSize: "11px" }}>{createError}</div>
            )}

            <div style={{ display: "flex", gap: "6px", marginTop: "2px" }}>
              <button
                type="submit" disabled={creating || !name.trim()}
                style={{
                  flex: 1, background: creating || !name.trim() ? "#3d4160" : "#5865f2",
                  color: "#fff", border: "none", borderRadius: "6px",
                  padding: "5px 0", fontSize: "12px",
                  cursor: creating || !name.trim() ? "default" : "pointer",
                }}
              >
                {creating ? "Creating…" : "Create"}
              </button>
              <button
                type="button"
                onClick={() => { setShowCreateForm(false); resetForm() }}
                style={{
                  background: "#2e3345", color: "#9a9fad", border: "none",
                  borderRadius: "6px", padding: "5px 8px", fontSize: "12px",
                  cursor: "pointer", display: "flex", alignItems: "center",
                }}
              >
                <Icon icon="lucide:x" style={{ fontSize: "13px" }} />
              </button>
            </div>
          </form>
        </div>
      )}

      {/* Channel list */}
      <div className="scrollbar-thin" style={{ flex: 1, overflowY: "auto", padding: "6px 0" }}>
        {!activeServerId ? (
          <div style={{ padding: "16px 14px", fontSize: "12px", color: "#5f6478" }}>
            Select or create a server from the left sidebar.
          </div>
        ) : loading ? (
          <div style={{ padding: "10px 14px", color: "#5f6478", fontSize: "12px" }}>Loading…</div>
        ) : (
          <>
            <SectionHeader>Text Channels</SectionHeader>
            {textChannels.length === 0 && (
              <div style={{ padding: "3px 14px", fontSize: "12px", color: "#5f6478" }}>
                No channels yet
              </div>
            )}
            {textChannels.map((room) => (
              <ChannelItem key={room.id} room={room} active={room.id === activeRoomId}
                onClick={() => onSelectRoom(room.id)} isAgent={false}
                isOwner={user?.id === activeServer?.created_by}
                onDelete={() => onRoomDeleted(room.id)} />
            ))}

            {agentRooms.length > 0 && (
              <>
                <SectionHeader style={{ marginTop: "6px" }}>Agent Rooms</SectionHeader>
                {agentRooms.map((room) => (
                  <ChannelItem key={room.id} room={room} active={room.id === activeRoomId}
                    onClick={() => onSelectRoom(room.id)} isAgent={true}
                    isOwner={user?.id === activeServer?.created_by}
                    onDelete={() => onRoomDeleted(room.id)} />
                ))}
              </>
            )}
          </>
        )}
      </div>

      {/* User panel */}
      {user && (
        <div
          style={{
            padding: "8px",
            borderTop: "1px solid #2e3240",
            display: "flex",
            alignItems: "center",
            gap: "8px",
          }}
        >
          <div
            style={{
              width: "28px", height: "28px", borderRadius: "50%",
              background: "#3b2f6e", color: "#a99ef0",
              display: "flex", alignItems: "center", justifyContent: "center",
              fontSize: "10px", fontWeight: "500", flexShrink: 0,
            }}
          >
            {user.username.slice(0, 2).toUpperCase()}
          </div>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ fontSize: "12px", fontWeight: "500", color: "#e0e2ea", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
              {user.username}
            </div>
            <div style={{ fontSize: "11px", color: "#3ba55d" }}>● Online</div>
          </div>
          <button
            onClick={onLogout} title="Sign out"
            style={{ background: "transparent", border: "none", color: "#9a9fad", cursor: "pointer", padding: "4px", borderRadius: "4px", display: "flex", alignItems: "center" }}
            onMouseEnter={(e) => { e.currentTarget.style.background = "#2e3345"; e.currentTarget.style.color = "#e0e2ea" }}
            onMouseLeave={(e) => { e.currentTarget.style.background = "transparent"; e.currentTarget.style.color = "#9a9fad" }}
          >
            <Icon icon="lucide:log-out" style={{ fontSize: "15px" }} />
          </button>
        </div>
      )}

      {/* Server Settings Modal */}
      {showServerSettings && activeServer && (
        <ServerSettings
          server={activeServer}
          isOwner={user?.id === activeServer.created_by}
          onClose={() => setShowServerSettings(false)}
        />
      )}
    </div>
  )
}

function SectionHeader({ children, style }: { children: string; style?: CSSProperties }) {
  return (
    <div
      style={{
        padding: "6px 14px 2px", fontSize: "11px", fontWeight: "500",
        color: "#5f6478", letterSpacing: "0.06em", textTransform: "uppercase",
        ...style,
      }}
    >
      {children}
    </div>
  )
}

function ChannelItem({ room, active, onClick, isAgent, isOwner, onDelete }: {
  room: Room; active: boolean; onClick: () => void; isAgent: boolean
  isOwner?: boolean; onDelete?: () => void
}) {
  const [hovered, setHovered] = useState(false)
  const [deleting, setDeleting] = useState(false)

  async function handleDelete(e: React.MouseEvent) {
    e.stopPropagation()
    if (!confirm(`Delete channel "${room.name}"? This cannot be undone.`)) return
    setDeleting(true)
    try {
      await deleteRoom(room.id)
      onDelete?.()
    } finally {
      setDeleting(false)
    }
  }

  return (
    <div
      onClick={onClick}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        padding: "4px 8px 4px 14px", display: "flex", alignItems: "center", gap: "7px",
        cursor: "pointer", borderRadius: "4px", margin: "0 4px",
        color: isAgent ? "#57f2b8" : active || hovered ? "#e0e2ea" : "#9a9fad",
        fontSize: "13px",
        background: active ? "#3b3f52" : hovered ? "#2e3345" : "transparent",
        fontWeight: active ? "500" : "400",
      }}
    >
      <span style={{ color: isAgent ? "#2dab7a" : "#5f6478", flexShrink: 0, display: "flex", alignItems: "center" }}>
        {isAgent
          ? <Icon icon="lucide:cpu" style={{ fontSize: "13px" }} />
          : <Icon icon="lucide:hash" style={{ fontSize: "14px" }} />
        }
      </span>
      <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", flex: 1 }}>
        {room.name}
      </span>
      {isOwner && hovered && (
        <button
          onClick={handleDelete}
          disabled={deleting}
          title="Delete channel"
          style={{
            background: "transparent", border: "none", color: "#ed4245",
            cursor: deleting ? "default" : "pointer", padding: "2px",
            borderRadius: "4px", display: "flex", alignItems: "center", flexShrink: 0,
            opacity: deleting ? 0.5 : 1,
          }}
          onMouseEnter={(e) => { e.currentTarget.style.background = "#3b1a1a" }}
          onMouseLeave={(e) => { e.currentTarget.style.background = "transparent" }}
        >
          <Icon icon="lucide:trash-2" style={{ fontSize: "12px" }} />
        </button>
      )}
    </div>
  )
}

const iconBtnStyle: CSSProperties = {
  width: "22px", height: "22px", borderRadius: "6px",
  background: "transparent", border: "none",
  color: "#9a9fad", cursor: "pointer",
  display: "flex", alignItems: "center", justifyContent: "center",
}

const inputStyle: CSSProperties = {
  width: "100%", background: "#1e2229",
  border: "1px solid #2e3240", borderRadius: "6px",
  color: "#e0e2ea", padding: "5px 8px", fontSize: "12px",
  outline: "none", fontFamily: "inherit",
}
