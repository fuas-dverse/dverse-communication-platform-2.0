import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Store } from "@tauri-apps/plugin-store";
import type { BotConfig, BotStatus, Personality, LlmBackend } from "../types";
import BotCard from "./BotCard";
import BotForm from "./BotForm";

export default function BotsTab() {
  const [bots, setBots] = useState<BotConfig[]>([]);
  const [statuses, setStatuses] = useState<Record<string, boolean>>({});
  const [showForm, setShowForm] = useState(false);
  const [editingBot, setEditingBot] = useState<BotConfig | null>(null);
  const [networkRouter, setNetworkRouter] = useState("tcp/localhost:7447");

  useEffect(() => {
    (async () => {
      const store = await Store.load("launcher-config.json");
      const savedBots = await store.get<BotConfig[]>("bots");
      if (savedBots) setBots(savedBots);
      const network = await store.get<{ routerAddress: string }>("network");
      if (network?.routerAddress) setNetworkRouter(network.routerAddress);
    })();
  }, []);

  const refreshStatuses = useCallback(async () => {
    try {
      const list = await invoke<BotStatus[]>("get_bot_statuses");
      const map: Record<string, boolean> = {};
      list.forEach((s) => { map[s.id] = s.running; });
      setStatuses(map);
    } catch (_) {}
  }, []);

  useEffect(() => {
    refreshStatuses();
    const interval = setInterval(refreshStatuses, 3000);
    return () => clearInterval(interval);
  }, [refreshStatuses]);

  async function saveBots(updated: BotConfig[]) {
    setBots(updated);
    const store = await Store.load("launcher-config.json");
    await store.set("bots", updated);
    await store.save();
  }

  async function handleSaveBot(bot: BotConfig) {
    const botWithRouter = { ...bot, zenohRouter: networkRouter };
    const existing = bots.findIndex((b) => b.id === bot.id);
    const updated =
      existing >= 0
        ? bots.map((b) => (b.id === bot.id ? botWithRouter : b))
        : [...bots, botWithRouter];
    await saveBots(updated);
    setShowForm(false);
    setEditingBot(null);
  }

  async function handleDelete(id: string) {
    await handleStop(id);
    await saveBots(bots.filter((b) => b.id !== id));
  }

  async function handleStart(bot: BotConfig) {
    try {
      await invoke("start_bot", { config: toRustConfig(bot) });
      await refreshStatuses();
    } catch (e) {
      alert(`Failed to start @${bot.name}: ${e}`);
    }
  }

  async function handleStop(id: string) {
    try {
      await invoke("stop_bot", { id });
      await refreshStatuses();
    } catch (_) {}
  }

  function toRustConfig(bot: BotConfig) {
    return {
      id: bot.id,
      name: bot.name,
      description: bot.description,
      personality: bot.personality,
      system_prompt: bot.systemPrompt,
      llm_backend: bot.llmBackend,
      ollama_url: bot.ollamaUrl,
      ollama_model: bot.ollamaModel,
      claude_api_key: bot.claudeApiKey,
      zenoh_router: bot.zenohRouter,
    };
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold text-gray-100">My Bots</h2>
          <p className="text-sm text-gray-400">
            Configure and launch bot agents that connect to Zenoh.
          </p>
        </div>
        <button
          onClick={() => { setEditingBot(null); setShowForm(true); }}
          className="btn-primary"
        >
          + Add Bot
        </button>
      </div>

      {bots.length === 0 && !showForm && (
        <div className="text-center py-16 text-gray-500">
          No bots yet. Add one to get started.
        </div>
      )}

      <div className="grid gap-4">
        {bots.map((bot) => (
          <BotCard
            key={bot.id}
            bot={bot}
            running={statuses[bot.id] ?? false}
            onStart={() => handleStart(bot)}
            onStop={() => handleStop(bot.id)}
            onEdit={() => { setEditingBot(bot); setShowForm(true); }}
            onDelete={() => handleDelete(bot.id)}
          />
        ))}
      </div>

      {showForm && (
        <BotForm
          initial={editingBot}
          onSave={handleSaveBot}
          onCancel={() => { setShowForm(false); setEditingBot(null); }}
        />
      )}
    </div>
  );
}
