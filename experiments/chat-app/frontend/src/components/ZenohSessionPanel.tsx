import { useState, useEffect, type FormEvent } from "react"
import { Icon } from "@iconify/react"
import {
  getZenohSession,
  connectZenohSession,
  disconnectZenohSession,
  type ZenohSessionInfo,
} from "../api/zenoh"

type Mode = "string" | "manual"

export default function ZenohSessionPanel() {
  const [info, setInfo] = useState<ZenohSessionInfo | null>(null)
  const [showForm, setShowForm] = useState(false)
  const [mode, setMode] = useState<Mode>("string")
  const [connectionString, setConnectionString] = useState("")
  const [router, setRouter] = useState("")
  const [namespace, setNamespace] = useState("")
  const [busy, setBusy] = useState(false)
  const [connecting, setConnecting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    getZenohSession()
      .then(setInfo)
      .catch(() => setInfo({ connected: false, namespace: "" }))
  }, [])

  // Poll until connected after a connect attempt
  useEffect(() => {
    if (!connecting) return
    const id = setInterval(() => {
      getZenohSession().then((s) => {
        setInfo(s)
        if (s.connected) setConnecting(false)
      }).catch(() => {})
    }, 1000)
    const timeout = setTimeout(() => {
      setConnecting(false)
      setError("Connection timed out")
    }, 10000)
    return () => { clearInterval(id); clearTimeout(timeout) }
  }, [connecting])

  async function handleConnect(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      const opts =
        mode === "string"
          ? { connection_string: connectionString.trim() }
          : { router: router.trim(), namespace: namespace.trim() }
      await connectZenohSession(opts)
      setShowForm(false)
      setConnecting(true)
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to connect")
    } finally {
      setBusy(false)
    }
  }

  async function handleDisconnect() {
    setBusy(true)
    try {
      await disconnectZenohSession()
      setInfo({ connected: false, namespace: "" })
    } catch {
    } finally {
      setBusy(false)
    }
  }

  const connected = info?.connected ?? false
  const submitDisabled =
    busy ||
    (mode === "string" ? !connectionString.trim() : !router.trim())

  return (
    <div
      style={{
        padding: "10px 12px",
        borderTop: "1px solid #2e3240",
        background: "#111318",
      }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          marginBottom: connected || showForm ? "8px" : 0,
        }}>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: "6px",
            fontSize: "11px",
            fontWeight: "600",
            color: "#5f6478",
            letterSpacing: "0.05em",
            textTransform: "uppercase",
          }}>
          <Icon icon="lucide:hexagon" style={{ fontSize: "12px" }} />
          DVerse
        </div>
        {connected ? (
          <button
            onClick={handleDisconnect}
            disabled={busy}
            title="Disconnect from DVerse"
            style={iconBtn}>
            <Icon icon="lucide:unplug" style={{ fontSize: "13px" }} />
          </button>
        ) : (
          <button
            onClick={() => setShowForm((v) => !v)}
            title="Connect to DVerse session"
            style={{ ...iconBtn, color: showForm ? "#57f2b8" : "#5f6478" }}>
            <Icon
              icon={showForm ? "lucide:x" : "lucide:plug"}
              style={{ fontSize: "13px" }}
            />
          </button>
        )}
      </div>

      {connecting && !connected && (
        <div style={{ display: "flex", alignItems: "center", gap: "6px", fontSize: "11px" }}>
          <span style={{ width: "6px", height: "6px", borderRadius: "50%", background: "#e0c07d", flexShrink: 0 }} />
          <span style={{ color: "#e0c07d" }}>Connecting…</span>
        </div>
      )}

      {connected && info && (
        <div style={{ display: "flex", alignItems: "center", gap: "6px", fontSize: "11px" }}>
          <span style={{ width: "6px", height: "6px", borderRadius: "50%", background: "#57f2b8", flexShrink: 0 }} />
          <span style={{ color: "#57f2b8", fontWeight: "500" }}>Connected</span>
          {info.namespace && (
            <span style={{ color: "#5f6478", fontFamily: "monospace", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
              · {info.namespace}
            </span>
          )}
        </div>
      )}

      {showForm && !connected && (
        <form onSubmit={handleConnect} style={{ display: "flex", flexDirection: "column", gap: "6px" }}>
          {/* Mode toggle */}
          <div style={{ display: "flex", gap: "4px" }}>
            {(["string", "manual"] as Mode[]).map((m) => (
              <button
                key={m}
                type="button"
                onClick={() => setMode(m)}
                style={{
                  flex: 1,
                  background: mode === m ? "#2e3345" : "transparent",
                  border: `1px solid ${mode === m ? "#3d4160" : "#2e3240"}`,
                  borderRadius: "5px",
                  color: mode === m ? "#e0e2ea" : "#5f6478",
                  fontSize: "10px",
                  padding: "3px 0",
                  cursor: "pointer",
                  fontWeight: mode === m ? "600" : "400",
                }}>
                {m === "string" ? "Connection string" : "Manual"}
              </button>
            ))}
          </div>

          {mode === "string" ? (
            <textarea
              autoFocus
              value={connectionString}
              onChange={(e) => setConnectionString(e.target.value)}
              placeholder="Paste connection string from DVerse Bridge tab…"
              rows={3}
              style={{ ...inputStyle, resize: "none", lineHeight: "1.4", fontFamily: "monospace", fontSize: "10px" }}
            />
          ) : (
            <>
              <input
                autoFocus
                type="text"
                value={router}
                onChange={(e) => setRouter(e.target.value)}
                placeholder="tcp/192.168.1.10:7447"
                style={inputStyle}
              />
              <input
                type="text"
                value={namespace}
                onChange={(e) => setNamespace(e.target.value)}
                placeholder="Session namespace (optional)"
                style={inputStyle}
              />
            </>
          )}

          {error && <div style={{ color: "#ed4245", fontSize: "11px" }}>{error}</div>}

          <button
            type="submit"
            disabled={submitDisabled}
            style={{
              background: submitDisabled ? "#2e3345" : "#57f2b8",
              color: submitDisabled ? "#5f6478" : "#111318",
              border: "none",
              borderRadius: "6px",
              padding: "6px 0",
              fontSize: "12px",
              fontWeight: "600",
              cursor: submitDisabled ? "default" : "pointer",
            }}>
            {busy ? "Connecting…" : "Connect"}
          </button>
        </form>
      )}
    </div>
  )
}

const iconBtn: React.CSSProperties = {
  background: "transparent",
  border: "none",
  color: "#5f6478",
  cursor: "pointer",
  padding: "2px",
  display: "flex",
  alignItems: "center",
}

const inputStyle: React.CSSProperties = {
  width: "100%",
  background: "#1e2229",
  border: "1px solid #2e3240",
  borderRadius: "6px",
  color: "#e0e2ea",
  padding: "5px 8px",
  fontSize: "11px",
  outline: "none",
  fontFamily: "inherit",
  boxSizing: "border-box",
}
