import { useState, useEffect, useRef, useCallback } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { useAuth } from '../store/auth'
import { getRooms, getRoom } from '../api/rooms'
import { getMessages, sendMessage } from '../api/messages'
import {
  getServers,
  getServerMembers,
  streamServer,
  joinServer,
} from '../api/servers'
import { API_BASE, getToken } from '../api/client'
import SpacesRail from '../components/SpacesRail'
import ChannelsSidebar from '../components/ChannelsSidebar'
import ChatArea from '../components/ChatArea'
import MembersPanel from '../components/MembersPanel'
import type { Room, Message, BotConfig, Server, ServerMember } from '../types'

export default function MainPage() {
  const { id: roomId } = useParams<{ id: string }>()
  const { user, logout } = useAuth()
  const navigate = useNavigate()

  // ── Server state ──────────────────────────────────────────────
  const [servers, setServers] = useState<Server[]>([])
  const [activeServerId, setActiveServerId] = useState<string | null>(null)
  const [serverMembers, setServerMembers] = useState<ServerMember[]>([])

  // ── Room/channel state ────────────────────────────────────────
  const [rooms, setRooms] = useState<Room[]>([])
  const [activeRoom, setActiveRoom] = useState<Room | null>(null)
  const [messages, setMessages] = useState<Message[]>([])
  const [loadingRooms, setLoadingRooms] = useState(false)
  const [loadingMessages, setLoadingMessages] = useState(false)

  const sseServerAbort = useRef<AbortController | null>(null)
  const sseMsgsAbort = useRef<AbortController | null>(null)

  // ── Load servers on mount ──────────────────────────────────────
  useEffect(() => {
    getServers()
      .then((list) => {
        setServers(list)
        // Auto-select first server
        if (list.length > 0) setActiveServerId(list[0].id)
      })
      .catch(() => setServers([]))
  }, [])

  // ── Load rooms when active server changes ──────────────────────
  useEffect(() => {
    if (!activeServerId) {
      setRooms([])
      return
    }
    setLoadingRooms(true)
    getRooms(activeServerId)
      .then(setRooms)
      .catch(() => setRooms([]))
      .finally(() => setLoadingRooms(false))
  }, [activeServerId])

  // ── Load server members when active server changes ─────────────
  useEffect(() => {
    if (!activeServerId) {
      setServerMembers([])
      return
    }

    let cancelled = false

    const fetchMembers = () => {
      getServerMembers(activeServerId)
        .then((members) => {
          if (!cancelled) setServerMembers(members)
        })
        .catch(() => {
          if (!cancelled) setServerMembers([])
        })
    }

    fetchMembers()
    const intervalId = window.setInterval(fetchMembers, 10000)

    return () => {
      cancelled = true
      window.clearInterval(intervalId)
    }
  }, [activeServerId])

  // ── Mark current user as online when viewing server ──────────────
  useEffect(() => {
    if (!activeServerId || !user || !getToken()) return
    // Call join to mark user as online (idempotent if already joined)
    joinServer(activeServerId).catch(() => {})
  }, [activeServerId, user])

  // ── SSE for server events (new rooms, new members) ─────────────
  useEffect(() => {
    if (!activeServerId) return

    const ctrl = new AbortController()
    sseServerAbort.current = ctrl

    async function connect() {
      try {
        const res = await streamServer(activeServerId!, ctrl.signal)
        if (!res.ok || !res.body) {
          setTimeout(connect, 2000)
          return
        }

        const reader = res.body.getReader()
        const decoder = new TextDecoder()
        let buf = ''

        while (true) {
          const { done, value } = await reader.read()
          if (done) {
            setTimeout(connect, 1000)
            break
          }
          buf += decoder.decode(value, { stream: true })
          const parts = buf.split('\n\n')
          buf = parts.pop() ?? ''
          for (const part of parts) {
            const dataLine = part.split('\n').find((l) => l.startsWith('data:'))
            if (!dataLine) continue
            try {
              const payload = JSON.parse(dataLine.slice(5).trim())
              if (payload.type === 'room_created') {
                setRooms((prev) => {
                  if (prev.some((r) => r.id === payload.room.id)) return prev
                  return [...prev, payload.room]
                })
              } else if (payload.type === 'room_deleted') {
                setRooms((prev) => prev.filter((r) => r.id !== payload.room_id))
              } else if (payload.type === 'member_joined') {
                setServerMembers((prev) => {
                  if (prev.some((m) => m.user_id === payload.member.user_id))
                    return prev
                  return [...prev, payload.member]
                })
              } else if (payload.type === 'member_online') {
                setServerMembers((prev) =>
                  prev.map((m) =>
                    m.user_id === payload.user_id
                      ? {
                          ...m,
                          is_online: true,
                          last_seen: new Date().toISOString(),
                        }
                      : m,
                  ),
                )
              } else if (payload.type === 'member_offline') {
                setServerMembers((prev) =>
                  prev.map((m) =>
                    m.user_id === payload.user_id
                      ? { ...m, is_online: false, last_seen: payload.last_seen }
                      : m,
                  ),
                )
              }
            } catch {
              /* ignore */
            }
          }
        }
      } catch (e) {
        if (e instanceof Error && e.name === 'AbortError') return
        setTimeout(connect, 3000)
      }
    }

    connect()
    return () => ctrl.abort()
  }, [activeServerId])

  // ── Load active room when roomId or rooms change ───────────────
  useEffect(() => {
    if (!roomId) {
      setActiveRoom(null)
      setMessages([])
      return
    }
    const existing = rooms.find((r) => r.id === roomId)
    if (existing) {
      setActiveRoom(existing)
    } else {
      getRoom(roomId)
        .then(setActiveRoom)
        .catch(() => setActiveRoom(null))
    }
  }, [roomId, rooms])

  // ── Load messages when room changes ───────────────────────────
  useEffect(() => {
    if (!roomId) return
    setLoadingMessages(true)
    setMessages([])
    getMessages(roomId)
      .then(setMessages)
      .catch(() => setMessages([]))
      .finally(() => setLoadingMessages(false))
  }, [roomId])

  // ── Poll fallback for message reconciliation ──────────────────
  useEffect(() => {
    if (!roomId) return

    let cancelled = false

    const reconcileMessages = () => {
      getMessages(roomId)
        .then((latest) => {
          if (cancelled) return
          setMessages((prev) => {
            const byId = new Map(prev.map((m) => [m.id, m]))
            for (const msg of latest) byId.set(msg.id, msg)
            return Array.from(byId.values()).sort((a, b) =>
              a.created_at.localeCompare(b.created_at),
            )
          })
        })
        .catch(() => {
          // ignore poll errors; SSE remains primary
        })
    }

    const intervalId = window.setInterval(reconcileMessages, 5000)

    return () => {
      cancelled = true
      window.clearInterval(intervalId)
    }
  }, [roomId])

  // ── SSE for messages ───────────────────────────────────────────
  const connectMsgsSSE = useCallback(async () => {
    if (!roomId) return
    const token = getToken()
    if (!token) return

    sseMsgsAbort.current?.abort()
    const controller = new AbortController()
    sseMsgsAbort.current = controller

    try {
      const response = await fetch(`${API_BASE}/rooms/${roomId}/stream`, {
        headers: { Authorization: `Bearer ${token}` },
        signal: controller.signal,
      })

      if (!response.ok || !response.body) {
        setTimeout(() => {
          if (!controller.signal.aborted) connectMsgsSSE()
        }, 2000)
        return
      }

      const reader = response.body.getReader()
      const decoder = new TextDecoder()
      let buffer = ''

      while (true) {
        const { done, value } = await reader.read()
        if (done) {
          setTimeout(() => {
            if (!controller.signal.aborted) connectMsgsSSE()
          }, 1000)
          break
        }
        buffer += decoder.decode(value, { stream: true })
        const parts = buffer.split('\n\n')
        buffer = parts.pop() ?? ''
        for (const part of parts) {
          const lines = part.split('\n')
          let data: string | null = null
          for (const line of lines) {
            if (line.startsWith('data: ')) data = line.slice(6)
          }
          if (!data) continue
          try {
            const event = JSON.parse(data) as {
              type: 'message' | 'replace'
              message: Message
            }
            if (event.type === 'message') {
              setMessages((prev) => {
                if (prev.some((m) => m.id === event.message.id)) return prev
                return [...prev, event.message]
              })
            } else if (event.type === 'replace') {
              setMessages((prev) =>
                prev.map((m) =>
                  m.id === event.message.id ? event.message : m,
                ),
              )
            }
          } catch {
            /* ignore */
          }
        }
      }
    } catch (err) {
      if (err instanceof Error && err.name === 'AbortError') return
      setTimeout(() => {
        if (!sseMsgsAbort.current?.signal.aborted) connectMsgsSSE()
      }, 3000)
    }
  }, [roomId])

  useEffect(() => {
    if (roomId) connectMsgsSSE()
    else sseMsgsAbort.current?.abort()
    return () => {
      sseMsgsAbort.current?.abort()
    }
  }, [connectMsgsSSE, roomId])

  // ── Handlers ──────────────────────────────────────────────────
  async function handleSend(content: string) {
    if (!roomId) return
    await sendMessage(roomId, content)
  }

  function handleBotsChange(bots: BotConfig[]) {
    setActiveRoom((prev) => (prev ? { ...prev, bots } : prev))
    setRooms((prev) => prev.map((r) => (r.id === roomId ? { ...r, bots } : r)))
  }

  function handleServerCreated(server: Server) {
    setServers((prev) => [...prev, server])
    setActiveServerId(server.id)
    navigate('/rooms')
  }

  function handleServerSelect(id: string) {
    setActiveServerId(id)
    navigate('/rooms')
  }

  function handleRoomCreated(room: Room) {
    navigate(`/rooms/${room.id}`)
  }

  function handleRoomDeleted(deletedRoomId: string) {
    setRooms((prev) => prev.filter((r) => r.id !== deletedRoomId))
    if (roomId === deletedRoomId) navigate('/rooms')
  }

  return (
    <div
      style={{
        display: 'flex',
        height: '100vh',
        overflow: 'hidden',
        background: '#282d38',
      }}>
      <SpacesRail
        servers={servers}
        activeServerId={activeServerId}
        onSelectServer={handleServerSelect}
        onServerCreated={handleServerCreated}
      />
      <ChannelsSidebar
        rooms={rooms}
        activeRoomId={roomId ?? null}
        activeServerId={activeServerId}
        activeServer={servers.find((s) => s.id === activeServerId) ?? null}
        user={user}
        onSelectRoom={(id) => navigate(`/rooms/${id}`)}
        onRoomCreated={handleRoomCreated}
        onRoomDeleted={handleRoomDeleted}
        onLogout={logout}
        loading={loadingRooms}
      />
      <ChatArea
        room={activeRoom}
        messages={messages}
        loadingMessages={loadingMessages}
        currentUserId={user?.id ?? ''}
        onSend={handleSend}
        onBotsChange={handleBotsChange}
        isCreator={user?.id === activeRoom?.created_by}
      />
      <MembersPanel
        room={activeRoom}
        serverMembers={serverMembers}
        currentUserId={user?.id ?? ''}
      />
    </div>
  )
}
