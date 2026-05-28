import { useState } from "react";
import type { BotConfig, Personality, LlmBackend } from "../types";
function generateId() {
  return Math.random().toString(36).slice(2) + Date.now().toString(36);
}

const PERSONALITIES: Personality[] = ["assistant", "coder", "creative", "analyst"];
const PERSONALITY_DESCRIPTIONS: Record<Personality, string> = {
  assistant: "Helpful, concise general assistant",
  coder: "Expert software engineer, code-focused",
  creative: "Imaginative storyteller and creative thinker",
  analyst: "Data-driven, critical analyst",
};

const DEFAULT_SYSTEM_PROMPTS: Record<Personality, string> = {
  assistant: "You are a helpful, concise assistant in a group chat.",
  coder: "You are an expert software engineer. Favor code and technical precision.",
  creative: "You are a creative thinker and storyteller. Be imaginative.",
  analyst: "You are a sharp analyst. Be data-driven and critical.",
};

interface Props {
  initial: BotConfig | null;
  onSave: (bot: BotConfig) => void;
  onCancel: () => void;
}

export default function BotForm({ initial, onSave, onCancel }: Props) {
  const [name, setName] = useState(initial?.name ?? "");
  const [description, setDescription] = useState(initial?.description ?? "");
  const [personality, setPersonality] = useState<Personality>(initial?.personality ?? "assistant");
  const [systemPrompt, setSystemPrompt] = useState(
    initial?.systemPrompt ?? DEFAULT_SYSTEM_PROMPTS["assistant"]
  );
  const [llmBackend, setLlmBackend] = useState<LlmBackend>(initial?.llmBackend ?? "ollama");
  const [ollamaUrl, setOllamaUrl] = useState(initial?.ollamaUrl ?? "http://localhost:11434");
  const [ollamaModel, setOllamaModel] = useState(initial?.ollamaModel ?? "deepseek-r1:1.5b");
  const [claudeApiKey, setClaudeApiKey] = useState(initial?.claudeApiKey ?? "");

  function handlePersonalityChange(p: Personality) {
    setPersonality(p);
    if (!initial) setSystemPrompt(DEFAULT_SYSTEM_PROMPTS[p]);
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!name.trim()) return;

    onSave({
      id: initial?.id ?? generateId(),
      name: name.trim().replace(/^@/, ""),
      description,
      personality,
      systemPrompt,
      llmBackend,
      ollamaUrl,
      ollamaModel,
      claudeApiKey,
      zenohRouter: initial?.zenohRouter ?? "",
    });
  }

  return (
    <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50 p-4">
      <div className="bg-gray-900 border border-gray-800 rounded-xl w-full max-w-lg max-h-[90vh] overflow-y-auto">
        <div className="p-6 border-b border-gray-800">
          <h3 className="font-semibold text-gray-100">
            {initial ? `Edit @${initial.name}` : "Add Bot"}
          </h3>
        </div>

        <form onSubmit={handleSubmit} className="p-6 space-y-5">
          <Field label="Bot Name" hint="Used as @mention in chat">
            <div className="flex items-center">
              <span className="text-gray-500 mr-1">@</span>
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                className="input flex-1"
                placeholder="mybot"
                required
              />
            </div>
          </Field>

          <Field label="Description" hint="Shown in the bot picker">
            <input
              type="text"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              className="input"
              placeholder="A helpful coding assistant"
            />
          </Field>

          <Field label="Personality">
            <div className="grid grid-cols-2 gap-2">
              {PERSONALITIES.map((p) => (
                <button
                  key={p}
                  type="button"
                  onClick={() => handlePersonalityChange(p)}
                  className={`text-left px-3 py-2 rounded-lg border text-sm transition-colors ${
                    personality === p
                      ? "border-zenoh-500 bg-zenoh-900/30 text-zenoh-300"
                      : "border-gray-700 text-gray-400 hover:border-gray-600"
                  }`}
                >
                  <div className="font-medium capitalize">{p}</div>
                  <div className="text-xs text-gray-500 mt-0.5">{PERSONALITY_DESCRIPTIONS[p]}</div>
                </button>
              ))}
            </div>
          </Field>

          <Field label="System Prompt" hint="Individual instructions for this bot">
            <textarea
              value={systemPrompt}
              onChange={(e) => setSystemPrompt(e.target.value)}
              className="input min-h-[80px] resize-y"
              placeholder="You are..."
            />
          </Field>

          <Field label="LLM Backend">
            <div className="flex gap-2">
              {(["ollama", "claude"] as LlmBackend[]).map((b) => (
                <button
                  key={b}
                  type="button"
                  onClick={() => setLlmBackend(b)}
                  className={`px-4 py-1.5 rounded-md text-sm font-medium border transition-colors ${
                    llmBackend === b
                      ? "border-zenoh-500 bg-zenoh-900/30 text-zenoh-300"
                      : "border-gray-700 text-gray-400 hover:border-gray-600"
                  }`}
                >
                  {b === "ollama" ? "Ollama (local)" : "Claude API"}
                </button>
              ))}
            </div>
          </Field>

          {llmBackend === "ollama" ? (
            <>
              <Field label="Ollama URL">
                <input
                  type="text"
                  value={ollamaUrl}
                  onChange={(e) => setOllamaUrl(e.target.value)}
                  className="input"
                  placeholder="http://localhost:11434"
                />
              </Field>
              <Field label="Model">
                <input
                  type="text"
                  value={ollamaModel}
                  onChange={(e) => setOllamaModel(e.target.value)}
                  className="input"
                  placeholder="deepseek-r1:1.5b"
                />
              </Field>
            </>
          ) : (
            <Field label="Anthropic API Key">
              <input
                type="password"
                value={claudeApiKey}
                onChange={(e) => setClaudeApiKey(e.target.value)}
                className="input"
                placeholder="sk-ant-..."
              />
            </Field>
          )}

          <div className="flex gap-3 pt-2">
            <button type="submit" className="btn-primary flex-1">
              {initial ? "Save Changes" : "Add Bot"}
            </button>
            <button type="button" onClick={onCancel} className="btn-secondary">
              Cancel
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-1">
      <label className="text-sm font-medium text-gray-300">{label}</label>
      {children}
      {hint && <p className="text-xs text-gray-500">{hint}</p>}
    </div>
  );
}
