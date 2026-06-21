import { useState, useEffect } from "react"
import { addBot, deleteBot } from "../api/rooms"
import { getZenohNodes, type DverseNode } from "../api/zenoh"
import type { BotConfig } from "../types"

interface Props {
  roomId: string
  bots: BotConfig[]
  onBotsChange: (bots: BotConfig[]) => void
}

export default function BotSettings({ roomId, bots, onBotsChange }: Props) {
  const [nodes, setNodes] = useState<DverseNode[]>([])
  const [adding, setAdding] = useState<string | null>(null)
  const [deletingId, setDeletingId] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    getZenohNodes().then(setNodes).catch(() => setNodes([]))
    const id = setInterval(() => getZenohNodes().then(setNodes).catch(() => {}), 5000)
    return () => clearInterval(id)
  }, [])

  const availableAgents: { node: string; agent: string }[] = nodes.flatMap((n) =>
    n.agents.map((a) => ({ node: n.name, agent: a }))
  )
  const addedNames = new Set(bots.map((b) => b.name.toLowerCase()))

  async function handleAdd(agentName: string) {
    setAdding(agentName)
    setError(null)
    try {
      const newBot = await addBot(roomId, {
        name: agentName,
        provider: "zenoh",
        personality: "assistant",
      })
      onBotsChange([...bots, newBot])
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to add agent")
    } finally {
      setAdding(null)
    }
  }

  async function handleDelete(botId: string) {
    setDeletingId(botId)
    try {
      await deleteBot(roomId, botId)
      onBotsChange(bots.filter((b) => b.id !== botId))
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to remove agent")
    } finally {
      setDeletingId(null)
    }
  }

  return (
    <div style={{ background: "#1e2229", borderBottom: "1px solid #2e3240", padding: "12px 16px" }}>
      {bots.length > 0 && (
        <div style={{ marginBottom: "10px" }}>
          <div style={{ fontSize: "11px", color: "#5f6478", textTransform: "uppercase", letterSpacing: "0.05em", marginBottom: "6px" }}>
            Active in this room
          </div>
          <div style={{ display: "flex", flexWrap: "wrap", gap: "6px" }}>
            {bots.map((bot) => (
              <div
                key={bot.id}
                style={{
                  display: "flex", alignItems: "center", gap: "6px",
                  background: "#163524", border: "1px solid #2a4a3a",
                  borderRadius: "6px", padding: "4px 8px", fontSize: "12px",
                }}
              >
                <span style={{ color: "#57f2b8" }}>@{bot.name}</span>
                <button
                  onClick={() => handleDelete(bot.id)}
                  disabled={deletingId === bot.id}
                  style={{ background: "transparent", border: "none", color: "#5f6478", cursor: "pointer", padding: "0", lineHeight: 1, fontSize: "14px" }}
                >
                  ×
                </button>
              </div>
            ))}
          </div>
        </div>
      )}

      <div>
        <div style={{ fontSize: "11px", color: "#5f6478", textTransform: "uppercase", letterSpacing: "0.05em", marginBottom: "6px" }}>
          DVerse agents {nodes.length === 0 ? "(no session connected)" : `— ${availableAgents.length} available`}
        </div>
        {availableAgents.length === 0 ? (
          <div style={{ fontSize: "12px", color: "#5f6478" }}>
            {nodes.length === 0
              ? "Connect to a DVerse session from the sidebar to see agents."
              : "No agents online in this session."}
          </div>
        ) : (
          <div style={{ display: "flex", flexWrap: "wrap", gap: "6px" }}>
            {availableAgents.map(({ node, agent }) => {
              const alreadyAdded = addedNames.has(agent.toLowerCase())
              return (
                <button
                  key={`${node}/${agent}`}
                  onClick={() => !alreadyAdded && handleAdd(agent)}
                  disabled={alreadyAdded || adding === agent}
                  title={`From node: ${node}`}
                  style={{
                    background: alreadyAdded ? "#163524" : "#2e3345",
                    border: `1px solid ${alreadyAdded ? "#2a4a3a" : "#3d4160"}`,
                    borderRadius: "6px", padding: "4px 10px",
                    fontSize: "12px", cursor: alreadyAdded ? "default" : "pointer",
                    color: alreadyAdded ? "#57f2b8" : "#9a9fad",
                    display: "flex", alignItems: "center", gap: "4px",
                  }}
                >
                  {alreadyAdded ? "✓" : adding === agent ? "…" : "+"} @{agent}
                </button>
              )
            })}
          </div>
        )}
      </div>

      {error && (
        <div style={{ color: "#ed4245", fontSize: "11px", marginTop: "8px" }}>{error}</div>
      )}
    </div>
  )
}
