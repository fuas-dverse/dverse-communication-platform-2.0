import { useState, useEffect } from "react"
import { invoke } from "@tauri-apps/api/core"

interface BridgeInfo {
  running: boolean
  ws_port: number | null
  connection_string: string | null
  token: string | null
}

export default function BridgeTab() {
  const [info, setInfo] = useState<BridgeInfo | null>(null)
  const [busy, setBusy] = useState(false)
  const [copied, setCopied] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    invoke<BridgeInfo>("get_bridge_info").then(setInfo).catch(() => {})
  }, [])

  async function handleStart() {
    setBusy(true)
    setError(null)
    try {
      const result = await invoke<BridgeInfo>("start_bridge")
      setInfo(result)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  async function handleStop() {
    setBusy(true)
    try {
      const result = await invoke<BridgeInfo>("stop_bridge")
      setInfo(result)
    } finally {
      setBusy(false)
    }
  }

  async function handleCopy() {
    if (!info?.connection_string) return
    await navigator.clipboard.writeText(info.connection_string)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  return (
    <div className="max-w-lg space-y-6">
      <div>
        <h2 className="text-lg font-semibold text-gray-100 mb-1">DVerse Bridge</h2>
        <p className="text-sm text-gray-400">
          Start a local bridge bot that connects to the current DVerse session via mTLS and
          exposes plaintext messages over a local WebSocket. Third-party apps paste the
          connection string to receive and send messages without needing certificates.
        </p>
      </div>

      <div className="flex items-center gap-3">
        <div
          className={`w-2.5 h-2.5 rounded-full ${info?.running ? "bg-green-400" : "bg-gray-600"}`}
        />
        <span className="text-sm text-gray-300">
          {info?.running ? `Running on port ${info.ws_port}` : "Stopped"}
        </span>
        {info?.running ? (
          <button
            onClick={handleStop}
            disabled={busy}
            className="ml-auto px-3 py-1.5 rounded-md text-xs font-medium bg-red-900 text-red-200 hover:bg-red-800 disabled:opacity-50 transition-colors"
          >
            Stop bridge
          </button>
        ) : (
          <button
            onClick={handleStart}
            disabled={busy}
            className="ml-auto px-3 py-1.5 rounded-md text-xs font-medium bg-zenoh-700 text-white hover:bg-zenoh-600 disabled:opacity-50 transition-colors"
          >
            {busy ? "Starting…" : "Start bridge"}
          </button>
        )}
      </div>

      {error && (
        <div className="text-sm text-red-400 bg-red-950 border border-red-800 rounded-md px-3 py-2">
          {error}
        </div>
      )}

      {info?.running && info.connection_string && (
        <div className="space-y-2">
          <label className="text-xs font-semibold text-gray-400 uppercase tracking-wide">
            Connection string
          </label>
          <div className="relative">
            <textarea
              readOnly
              value={info.connection_string}
              rows={3}
              className="w-full bg-gray-900 border border-gray-700 rounded-md px-3 py-2 text-xs font-mono text-gray-300 resize-none outline-none"
            />
            <button
              onClick={handleCopy}
              className="absolute top-2 right-2 px-2 py-1 rounded text-xs bg-gray-700 text-gray-200 hover:bg-gray-600 transition-colors"
            >
              {copied ? "Copied!" : "Copy"}
            </button>
          </div>
          <p className="text-xs text-gray-500">
            Paste this into the DVerse panel of any client app to connect via the bridge.
          </p>
        </div>
      )}

      <div className="text-xs text-gray-500 space-y-1 border-t border-gray-800 pt-4">
        <p className="font-medium text-gray-400">How it works</p>
        <ol className="list-decimal list-inside space-y-1">
          <li>Start the bridge (requires an active Admin session).</li>
          <li>Copy the connection string.</li>
          <li>
            In your client app → DVerse panel → paste in{" "}
            <em>Connection string</em> mode and click Connect.
          </li>
          <li>Messages flow through the bridge, Megolm-decrypted by the Tauri app.</li>
        </ol>
      </div>
    </div>
  )
}
