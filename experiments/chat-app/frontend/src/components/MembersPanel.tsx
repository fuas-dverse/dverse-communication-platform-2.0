import { useState } from 'react'
import type { Room, ServerMember } from '../types'

interface Props {
  room: Room | null
  serverMembers: ServerMember[]
  currentUserId: string
}

function hashColor(
  name: string,
  palette: Array<{ bg: string; color: string }>,
) {
  let hash = 0
  for (const ch of name) hash = (hash * 31 + ch.charCodeAt(0)) | 0
  return palette[Math.abs(hash) % palette.length]
}

const BOT_COLORS = [
  { bg: '#163524', color: '#57f2b8' },
  { bg: '#1a1f3a', color: '#7289da' },
  { bg: '#2d1a1a', color: '#e07d7d' },
  { bg: '#2d1a3a', color: '#c07df0' },
  { bg: '#1a2d1a', color: '#7dc87d' },
]

const USER_COLORS = [
  { bg: '#3b2f6e', color: '#a99ef0' },
  { bg: '#1f2d4a', color: '#7da7e0' },
  { bg: '#2d1a1a', color: '#e07d7d' },
  { bg: '#1a2d1a', color: '#7dc87d' },
  { bg: '#2d2a1a', color: '#e0c07d' },
]

function StatusDot({
  color,
  border = '#21252e',
}: {
  color: string
  border?: string
}) {
  return (
    <div
      style={{
        width: '8px',
        height: '8px',
        borderRadius: '50%',
        background: color,
        position: 'absolute',
        bottom: '-1px',
        right: '-1px',
        border: `2px solid ${border}`,
      }}
    />
  )
}

function SectionLabel({
  children,
  style,
}: {
  children: React.ReactNode
  style?: React.CSSProperties
}) {
  return (
    <div
      style={{
        padding: '6px 14px 4px',
        fontSize: '11px',
        fontWeight: '500',
        color: '#5f6478',
        letterSpacing: '0.06em',
        textTransform: 'uppercase',
        ...style,
      }}>
      {children}
    </div>
  )
}

function MemberRow({
  avatar,
  name,
  badge,
  dim = false,
}: {
  avatar: React.ReactNode
  name: React.ReactNode
  badge?: React.ReactNode
  dim?: boolean
}) {
  const [hovered, setHovered] = useState(false)
  return (
    <div
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        padding: '5px 14px',
        display: 'flex',
        alignItems: 'center',
        gap: '9px',
        cursor: 'pointer',
        opacity: dim ? 0.45 : 1,
        background: hovered ? '#2e3345' : 'transparent',
      }}>
      {avatar}
      <span style={{ fontSize: '13px' }}>{name}</span>
      {badge}
    </div>
  )
}

export default function MembersPanel({
  room,
  serverMembers,
  currentUserId,
}: Props) {
  const bots = room?.bots ?? []

  const onlineMembers = serverMembers.filter((m) => m.is_online)
  const offlineMembers = serverMembers.filter((m) => !m.is_online)

  return (
    <div
      style={{
        width: '220px',
        background: '#21252e',
        borderLeft: '1px solid #2e3240',
        display: 'flex',
        flexDirection: 'column',
        flexShrink: 0,
      }}>
      {/* Header */}
      <div
        style={{
          padding: '14px 14px 10px',
          borderBottom: '1px solid #2e3240',
          fontWeight: '500',
          fontSize: '14px',
          color: '#e0e2ea',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          flexShrink: 0,
        }}>
        Members
        {serverMembers.length > 0 && (
          <span
            style={{ fontSize: '12px', fontWeight: '400', color: '#5f6478' }}>
            {serverMembers.length + bots.length}
          </span>
        )}
      </div>

      <div
        className="scrollbar-thin"
        style={{ flex: 1, overflowY: 'auto', padding: '8px 0' }}>
        {serverMembers.length === 0 && bots.length === 0 ? (
          <div
            style={{
              padding: '20px 14px',
              fontSize: '12px',
              color: '#5f6478',
              textAlign: 'center',
            }}>
            Select a server to see members
          </div>
        ) : (
          <>
            {/* AI Agents */}
            {bots.length > 0 && (
              <>
                <SectionLabel>AI Agents — {bots.length}</SectionLabel>
                {bots.map((bot) => {
                  const { bg, color } = hashColor(bot.name, BOT_COLORS)
                  return (
                    <div key={bot.id}>
                      <MemberRow
                        avatar={
                          <div
                            style={{
                              width: '24px',
                              height: '24px',
                              borderRadius: '8px',
                              background: bg,
                              color,
                              display: 'flex',
                              alignItems: 'center',
                              justifyContent: 'center',
                              fontSize: '9px',
                              fontWeight: '500',
                              flexShrink: 0,
                              position: 'relative',
                            }}>
                            {bot.name.slice(0, 2).toUpperCase()}
                            <StatusDot color="#2dab7a" />
                          </div>
                        }
                        name={<span style={{ color }}>@{bot.name}</span>}
                        badge={
                          <span
                            style={{
                              marginLeft: 'auto',
                              fontSize: '10px',
                              padding: '1px 5px',
                              borderRadius: '4px',
                              background: '#163524',
                              color: '#57f2b8',
                              flexShrink: 0,
                            }}>
                            bot
                          </span>
                        }
                      />
                      <div
                        style={{
                          fontSize: '11px',
                          color: '#5f6478',
                          padding: '0 14px 4px',
                          lineHeight: '1.4',
                        }}>
                        {bot.personality} · {bot.provider}
                      </div>
                    </div>
                  )
                })}
              </>
            )}

            {/* Online (just current user for now) */}
            {onlineMembers.length > 0 && (
              <>
                <SectionLabel
                  style={{ marginTop: bots.length > 0 ? '8px' : undefined }}>
                  Online — {onlineMembers.length}
                </SectionLabel>
                {onlineMembers.map((m) => {
                  const { bg, color } = hashColor(m.username, USER_COLORS)
                  const isCurrentUser = m.user_id === currentUserId
                  return (
                    <MemberRow
                      key={m.user_id}
                      avatar={
                        <div
                          style={{
                            width: '24px',
                            height: '24px',
                            borderRadius: '50%',
                            background: bg,
                            color,
                            display: 'flex',
                            alignItems: 'center',
                            justifyContent: 'center',
                            fontSize: '9px',
                            fontWeight: '500',
                            flexShrink: 0,
                            position: 'relative',
                          }}>
                          {m.username.slice(0, 2).toUpperCase()}
                          <StatusDot color="#3ba55d" />
                        </div>
                      }
                      name={
                        <span style={{ color: '#9a9fad' }}>{m.username}</span>
                      }
                      badge={
                        isCurrentUser ? (
                          <span
                            style={{
                              marginLeft: 'auto',
                              fontSize: '10px',
                              padding: '1px 5px',
                              borderRadius: '4px',
                              background: '#444',
                              color: '#9a9fad',
                              flexShrink: 0,
                            }}>
                            you
                          </span>
                        ) : undefined
                      }
                    />
                  )
                })}
              </>
            )}

            {/* Offline members */}
            {offlineMembers.length > 0 && (
              <>
                <SectionLabel style={{ marginTop: '8px' }}>
                  Offline — {offlineMembers.length}
                </SectionLabel>
                {offlineMembers.map((m) => {
                  return (
                    <MemberRow
                      key={m.user_id}
                      dim
                      avatar={
                        <div
                          style={{
                            width: '24px',
                            height: '24px',
                            borderRadius: '50%',
                            background: '#222',
                            color: '#666',
                            display: 'flex',
                            alignItems: 'center',
                            justifyContent: 'center',
                            fontSize: '9px',
                            fontWeight: '500',
                            flexShrink: 0,
                          }}>
                          {m.username.slice(0, 2).toUpperCase()}
                        </div>
                      }
                      name={
                        <span style={{ color: '#5f6478' }}>{m.username}</span>
                      }
                    />
                  )
                })}
              </>
            )}
          </>
        )}
      </div>
    </div>
  )
}
