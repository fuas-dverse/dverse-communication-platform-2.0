export type ConnectionStatus = "disconnected" | "pending" | "approved" | "denied";

export interface NetworkConfig {
  routerAddress: string;
  username: string;
  assignedDns: string | null;
  status: ConnectionStatus;
}

export type LlmBackend = "ollama" | "claude";
export type Personality = "assistant" | "coder" | "creative" | "analyst";

export interface BotConfig {
  id: string;
  name: string;
  description: string;
  personality: Personality;
  systemPrompt: string;
  llmBackend: LlmBackend;
  ollamaUrl: string;
  ollamaModel: string;
  claudeApiKey: string;
  zenohRouter: string;
}

export interface BotStatus {
  id: string;
  running: boolean;
}
