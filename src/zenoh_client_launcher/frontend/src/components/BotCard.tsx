import type { BotConfig } from "../types";

interface Props {
  bot: BotConfig;
  running: boolean;
  onStart: () => void;
  onStop: () => void;
  onEdit: () => void;
  onDelete: () => void;
}

const PERSONALITY_EMOJI: Record<string, string> = {
  assistant: "🤖",
  coder: "💻",
  creative: "🎨",
  analyst: "📊",
};

export default function BotCard({ bot, running, onStart, onStop, onEdit, onDelete }: Props) {
  return (
    <div className="flex items-center gap-4 p-4 bg-gray-900 rounded-lg border border-gray-800">
      <div className="text-2xl">{PERSONALITY_EMOJI[bot.personality] ?? "🤖"}</div>

      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="font-mono font-semibold text-zenoh-400">@{bot.name}</span>
          <span className="text-xs text-gray-500 capitalize">{bot.personality}</span>
          <span className="text-xs text-gray-600">·</span>
          <span className="text-xs text-gray-500">{bot.llmBackend === "ollama" ? bot.ollamaModel : "Claude"}</span>
        </div>
        {bot.description && (
          <p className="text-sm text-gray-400 truncate mt-0.5">{bot.description}</p>
        )}
        <p className="text-xs text-gray-600 font-mono mt-1 truncate">{bot.zenohRouter}</p>
      </div>

      <div className="flex items-center gap-2 shrink-0">
        <span className={`text-xs font-medium ${running ? "text-green-400" : "text-gray-500"}`}>
          {running ? "● online" : "○ offline"}
        </span>
        {running ? (
          <button onClick={onStop} className="btn-danger text-xs px-3 py-1">Stop</button>
        ) : (
          <button onClick={onStart} className="btn-primary text-xs px-3 py-1">Start</button>
        )}
        <button onClick={onEdit} className="btn-secondary text-xs px-3 py-1">Edit</button>
        <button
          onClick={() => { if (confirm(`Delete @${bot.name}?`)) onDelete(); }}
          className="text-gray-500 hover:text-red-400 transition-colors text-xs px-2"
        >
          ✕
        </button>
      </div>
    </div>
  );
}
