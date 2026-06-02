// ── Network / discovery ────────────────────────────────────────────────────────

export type ConnectionStatus = "disconnected" | "pending" | "approved" | "denied";

export interface DiscoveredRouter {
  name: string;
  host: string;
  port: number;
  zenoh_addr: string;
  cn: string;
  session: string;
}

export interface NetworkConfig {
  zenohAddr: string;
  username: string;
  password: string;
  assignedDns: string | null;
  status: ConnectionStatus;
}

// ── Bot management ─────────────────────────────────────────────────────────────

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

// ── App state (mirrored from Rust backend) ─────────────────────────────────────

export type AppScreen = "login" | "register" | "loading" | "main";

export type RouterStatus =
  | "idle"
  | "acquiring"
  | "starting"
  | "running"
  | "reloading"
  | { error: string };

export type AgentStatus = "online" | "degraded" | "offline";

export interface AgentInfo {
  version: string;
  publishes: string[];
  subscribes: string[];
  status: AgentStatus;
  last_seen_secs_ago: number;
}

export interface NodeInfo {
  cn: string;
  agents: Record<string, AgentInfo>;
}

export type SessionRoleDto =
  | { kind: "admin" }
  | { kind: "client"; admin_cn: string };

export interface AppSnapshot {
  screen: AppScreen;
  router_status: RouterStatus;
  session_id: string;
  session_role: SessionRoleDto;
  connected_nodes: NodeInfo[];
  log: string[];
  error: string | null;
}
