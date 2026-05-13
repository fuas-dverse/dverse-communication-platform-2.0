import { useState, type FormEvent } from 'react'
import { createServer, joinServerByCode } from '../api/servers'
import type { Server } from '../types'
import { Icon } from '@iconify/react'

interface Props {
  servers: Server[]
  activeServerId: string | null
  onSelectServer: (id: string) => void
  onServerCreated: (server: Server) => void
}

function serverInitials(name: string): string {
  return (
    name
      .split(/\s+/)
      .slice(0, 2)
      .map((w) => w[0]?.toUpperCase() ?? '')
      .join('') || name.slice(0, 2).toUpperCase()
  )
}

function hashBg(name: string): string {
  const palette = [
    '#5865f2',
    '#2d7d46',
    '#c27c0e',
    '#a84300',
    '#ad1457',
    '#6a1b9a',
    '#00838f',
    '#1565c0',
  ]
  let h = 0
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) | 0
  return palette[Math.abs(h) % palette.length]
}

export default function SpacesRail({
  servers,
  activeServerId,
  onSelectServer,
  onServerCreated,
}: Props) {
  const [showForm, setShowForm] = useState(false)
  const [tab, setTab] = useState<'create' | 'join'>('create')
  // create
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  // join
  const [code, setCode] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  function closeForm() {
    setShowForm(false)
    setError(null)
    setName('')
    setDescription('')
    setCode('')
  }

  async function handleCreate(e: FormEvent) {
    e.preventDefault()
    if (!name.trim() || busy) return
    setBusy(true)
    setError(null)
    try {
      const server = await createServer({
        name: name.trim(),
        description: description.trim(),
      })
      onServerCreated(server)
      closeForm()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to create server')
    } finally {
      setBusy(false)
    }
  }

  async function handleJoin(e: FormEvent) {
    e.preventDefault()
    if (!code.trim() || busy) return
    setBusy(true)
    setError(null)
    try {
      const server = await joinServerByCode(code.trim())
      onServerCreated(server)
      closeForm()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Invalid invite code')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div
      style={{
        width: '64px',
        background: '#111318',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        padding: '10px 0',
        gap: '6px',
        borderRight: '1px solid #2e3240',
        flexShrink: 0,
        overflowY: 'auto',
        overflowX: 'hidden',
        position: 'relative',
      }}>
      {/* Server icons */}
      {servers.map((server) => (
        <SpaceIcon
          key={server.id}
          label={serverInitials(server.name)}
          title={server.name}
          active={server.id === activeServerId}
          bg={hashBg(server.name)}
          onClick={() => onSelectServer(server.id)}
        />
      ))}

      {servers.length > 0 && (
        <div
          style={{
            width: '32px',
            height: '1px',
            background: '#2e3240',
            margin: '2px 0',
            flexShrink: 0,
          }}
        />
      )}

      {/* Add / join server button */}
      {showForm ? (
        <div
          style={{
            position: 'fixed',
            left: '74px',
            top: '10px',
            background: '#1e2229',
            border: '1px solid #2e3240',
            borderRadius: '10px',
            padding: '12px',
            width: '230px',
            zIndex: 9998,
            display: 'flex',
            flexDirection: 'column',
            gap: '10px',
          }}>
          {/* Tabs */}
          <div style={{ display: 'flex', gap: '4px' }}>
            {(['create', 'join'] as const).map((t) => (
              <button
                key={t}
                type="button"
                onClick={() => {
                  setTab(t)
                  setError(null)
                }}
                style={{
                  flex: 1,
                  padding: '5px 0',
                  borderRadius: '6px',
                  border: 'none',
                  background: tab === t ? '#5865f2' : '#2e3345',
                  color: tab === t ? '#fff' : '#9a9fad',
                  fontSize: '11px',
                  fontWeight: tab === t ? '600' : '400',
                  cursor: 'pointer',
                }}>
                {t === 'create' ? 'Create' : 'Join'}
              </button>
            ))}
          </div>

          {tab === 'create' ? (
            <form
              onSubmit={handleCreate}
              style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
              <input
                autoFocus
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="Server name"
                required
                style={inputStyle}
              />
              <input
                type="text"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder="Description (optional)"
                style={inputStyle}
              />
              {error && (
                <div style={{ color: '#ed4245', fontSize: '11px' }}>
                  {error}
                </div>
              )}
              <div style={{ display: 'flex', gap: '6px' }}>
                <button
                  type="submit"
                  disabled={busy || !name.trim()}
                  style={submitBtnStyle(busy || !name.trim())}>
                  <Icon
                    icon="lucide:check"
                    style={{ fontSize: '13px' }}
                  />
                  {busy ? 'Creating…' : 'Create'}
                </button>
                <button
                  type="button"
                  onClick={closeForm}
                  style={cancelBtnStyle}>
                  <Icon
                    icon="lucide:x"
                    style={{ fontSize: '13px' }}
                  />
                </button>
              </div>
            </form>
          ) : (
            <form
              onSubmit={handleJoin}
              style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
              <div style={{ fontSize: '11px', color: '#5f6478' }}>
                Enter the invite code shared by the server owner.
              </div>
              <input
                autoFocus
                type="text"
                value={code}
                onChange={(e) => setCode(e.target.value.toUpperCase())}
                placeholder="e.g. A3F9B2C1"
                maxLength={8}
                required
                style={{
                  ...inputStyle,
                  letterSpacing: '0.15em',
                  fontFamily: 'monospace',
                }}
              />
              {error && (
                <div style={{ color: '#ed4245', fontSize: '11px' }}>
                  {error}
                </div>
              )}
              <div style={{ display: 'flex', gap: '6px' }}>
                <button
                  type="submit"
                  disabled={busy || code.trim().length < 8}
                  style={submitBtnStyle(busy || code.trim().length < 8)}>
                  <Icon
                    icon="lucide:log-in"
                    style={{ fontSize: '13px' }}
                  />
                  {busy ? 'Joining…' : 'Join'}
                </button>
                <button
                  type="button"
                  onClick={closeForm}
                  style={cancelBtnStyle}>
                  <Icon
                    icon="lucide:x"
                    style={{ fontSize: '13px' }}
                  />
                </button>
              </div>
            </form>
          )}
        </div>
      ) : (
        <div
          title="Create or join a server"
          onClick={() => setShowForm(true)}
          style={{
            width: '44px',
            height: '44px',
            borderRadius: '50%',
            background: '#1e2229',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            color: '#57f2b8',
            cursor: 'pointer',
            border: '1.5px dashed #2e3240',
            flexShrink: 0,
            userSelect: 'none',
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.borderRadius = '14px'
            e.currentTarget.style.background = '#2a3040'
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.borderRadius = '50%'
            e.currentTarget.style.background = '#1e2229'
          }}>
          <Icon
            icon="lucide:plus"
            style={{ fontSize: '20px' }}
          />
        </div>
      )}
    </div>
  )
}

function SpaceIcon({
  label,
  title,
  active,
  bg,
  onClick,
}: {
  label: string
  title: string
  active: boolean
  bg: string
  onClick: () => void
}) {
  const [hovered, setHovered] = useState(false)
  return (
    <div
      title={title}
      onClick={onClick}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        width: '44px',
        height: '44px',
        borderRadius: active || hovered ? '14px' : '50%',
        background: active ? bg : hovered ? '#2c3245' : '#1e2229',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        fontSize: '13px',
        fontWeight: '600',
        color: active ? '#fff' : '#9a9fad',
        cursor: 'pointer',
        position: 'relative',
        userSelect: 'none',
        flexShrink: 0,
        transition: 'border-radius 0.15s, background 0.15s',
        border: active ? 'none' : '1px solid #2e3240',
      }}>
      {label}
      {active && (
        <div
          style={{
            position: 'absolute',
            left: '-12px',
            width: '4px',
            height: '36px',
            background: '#e0e2ea',
            borderRadius: '0 4px 4px 0',
          }}
        />
      )}
    </div>
  )
}

const inputStyle: React.CSSProperties = {
  width: '100%',
  background: '#282d38',
  border: '1px solid #2e3240',
  borderRadius: '6px',
  color: '#e0e2ea',
  padding: '6px 8px',
  fontSize: '12px',
  outline: 'none',
  fontFamily: 'inherit',
}

function submitBtnStyle(disabled: boolean): React.CSSProperties {
  return {
    flex: 1,
    background: disabled ? '#3d4160' : '#5865f2',
    color: '#fff',
    border: 'none',
    borderRadius: '6px',
    padding: '6px 0',
    fontSize: '12px',
    cursor: disabled ? 'default' : 'pointer',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: '4px',
  }
}

const cancelBtnStyle: React.CSSProperties = {
  background: '#2e3345',
  color: '#9a9fad',
  border: 'none',
  borderRadius: '6px',
  padding: '6px 10px',
  fontSize: '12px',
  cursor: 'pointer',
  display: 'flex',
  alignItems: 'center',
}
