import { useState } from 'react'
import { generateInviteCode } from '../api/servers'
import type { Server } from '../types'
import { Icon } from '@iconify/react'

interface Props {
  server: Server | null
  isOwner: boolean
  onClose: () => void
}

export default function ServerSettings({ server, isOwner, onClose }: Props) {
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)
  const [localInviteCode, setLocalInviteCode] = useState(
    server?.invite_code || null,
  )

  if (!server) return null

  async function handleGenerateInvite() {
    if (!server || !isOwner || loading) return
    setLoading(true)
    setError(null)
    try {
      const updated = await generateInviteCode(server.id)
      setLocalInviteCode(updated.invite_code)
      setCopied(false)
    } catch (err) {
      setError(
        err instanceof Error ? err.message : 'Failed to generate invite code',
      )
    } finally {
      setLoading(false)
    }
  }

  function handleCopyInvite() {
    if (!localInviteCode) return
    navigator.clipboard.writeText(localInviteCode)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  return (
    <div
      style={{
        position: 'fixed',
        inset: 0,
        background: 'rgba(0, 0, 0, 0.6)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 9999,
      }}
      onClick={onClose}>
      <div
        style={{
          background: '#1e2229',
          border: '1px solid #2e3240',
          borderRadius: '12px',
          padding: '20px',
          width: '90%',
          maxWidth: '400px',
          boxShadow: '0 10px 40px rgba(0, 0, 0, 0.3)',
        }}
        onClick={(e) => e.stopPropagation()}>
        {/* Header */}
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            marginBottom: '16px',
          }}>
          <h2
            style={{
              fontSize: '16px',
              fontWeight: '600',
              color: '#e0e2ea',
              margin: 0,
            }}>
            {server.name}
          </h2>
          <button
            onClick={onClose}
            style={{
              background: 'transparent',
              border: 'none',
              color: '#9a9fad',
              cursor: 'pointer',
              padding: '4px',
              fontSize: '18px',
              display: 'flex',
              alignItems: 'center',
            }}>
            <Icon
              icon="lucide:x"
              style={{ fontSize: '18px' }}
            />
          </button>
        </div>

        {/* Server info */}
        <div
          style={{
            marginBottom: '16px',
            paddingBottom: '16px',
            borderBottom: '1px solid #2e3240',
          }}>
          {server.description && (
            <p
              style={{
                color: '#9a9fad',
                fontSize: '12px',
                margin: '0 0 8px 0',
                lineHeight: '1.4',
              }}>
              {server.description}
            </p>
          )}
          <div style={{ fontSize: '11px', color: '#5f6478' }}>
            Members:{' '}
            <span style={{ color: '#e0e2ea' }}>{server.member_count}</span>
          </div>
        </div>

        {/* Invite code section */}
        <div style={{ marginBottom: '16px' }}>
          <div
            style={{
              fontSize: '13px',
              fontWeight: '500',
              color: '#e0e2ea',
              marginBottom: '10px',
              display: 'flex',
              alignItems: 'center',
              gap: '6px',
            }}>
            <Icon
              icon="lucide:link"
              style={{ fontSize: '14px' }}
            />
            Invite Code
          </div>

          {!isOwner ? (
            <div
              style={{
                color: '#9a9fad',
                fontSize: '12px',
                padding: '8px',
                background: '#0f1218',
                borderRadius: '6px',
                border: '1px solid #2e3240',
              }}>
              Only the server owner can manage invites.
            </div>
          ) : (
            <div style={{ display: 'flex', gap: '8px' }}>
              <div
                style={{
                  flex: 1,
                  background: '#0f1218',
                  border: '1px solid #2e3240',
                  borderRadius: '6px',
                  padding: '8px 12px',
                  display: 'flex',
                  alignItems: 'center',
                  gap: '8px',
                }}>
                {localInviteCode ? (
                  <>
                    <code
                      style={{
                        flex: 1,
                        color: '#57f2b8',
                        fontSize: '13px',
                        fontWeight: '500',
                        letterSpacing: '0.1em',
                        fontFamily: 'monospace',
                      }}>
                      {localInviteCode}
                    </code>
                    <button
                      onClick={handleCopyInvite}
                      title={copied ? 'Copied!' : 'Copy invite code'}
                      style={{
                        background: 'transparent',
                        border: 'none',
                        color: copied ? '#3ba55d' : '#9a9fad',
                        cursor: 'pointer',
                        padding: '3px',
                        display: 'flex',
                        alignItems: 'center',
                        fontSize: '14px',
                      }}>
                      <Icon
                        icon={copied ? 'lucide:check' : 'lucide:copy'}
                        style={{ fontSize: '14px' }}
                      />
                    </button>
                  </>
                ) : (
                  <span style={{ color: '#5f6478', fontSize: '12px' }}>
                    No invite code yet
                  </span>
                )}
              </div>
            </div>
          )}

          {error && (
            <div
              style={{ color: '#ed4245', fontSize: '11px', marginTop: '6px' }}>
              {error}
            </div>
          )}
        </div>

        {/* Actions */}
        {isOwner && (
          <div style={{ display: 'flex', gap: '8px' }}>
            <button
              onClick={handleGenerateInvite}
              disabled={loading}
              style={{
                flex: 1,
                background: loading ? '#3d4160' : '#5865f2',
                color: '#fff',
                border: 'none',
                borderRadius: '6px',
                padding: '8px 12px',
                fontSize: '12px',
                fontWeight: '500',
                cursor: loading ? 'default' : 'pointer',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                gap: '6px',
              }}>
              <Icon
                icon={loading ? 'lucide:loader' : 'lucide:refresh-cw'}
                style={{ fontSize: '13px' }}
              />
              {loading ? 'Generating...' : 'Generate Invite'}
            </button>
          </div>
        )}

        {/* Join button */}
        <div
          style={{
            marginTop: '16px',
            paddingTop: '16px',
            borderTop: '1px solid #2e3240',
          }}>
          <p
            style={{
              fontSize: '11px',
              color: '#5f6478',
              margin: '0 0 8px 0',
              textTransform: 'uppercase',
              letterSpacing: '0.05em',
            }}>
            Share This Code
          </p>
          <p
            style={{
              fontSize: '12px',
              color: '#9a9fad',
              margin: 0,
              lineHeight: '1.4',
            }}>
            Others can join this server by entering the invite code in "Join"
            tab from the server creation modal.
          </p>
        </div>
      </div>
    </div>
  )
}
