export type ConnectionStatus = "disconnected" | "pending" | "approved" | "denied";

export interface DiscoveredRouter {
  name: string;
  host: string;
  port: number;
  zenohAddr: string;
}

export interface NetworkConfig {
  zenohAddr: string;       // selected router's tcp/host:port
  username: string;
  password: string;
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
