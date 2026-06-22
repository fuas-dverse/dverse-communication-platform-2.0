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

export type AppScreen =
  | "login"
  | "register"
  | "chooser"
  | "requesting_join"
  | "loading"
  | "main"
  | "kicked";

/// Member-side terminal state shown when this node received a KickNotice.
/// The Kicked screen renders `reason` and a "Back to chooser" action that
/// calls the `back_to_chooser` Tauri command.
export interface KickedDto {
  reason: string | null;
  banned: boolean;
}

/// Requester-side join-flow status.
export type JoinFlowDto =
  | { status: "pending"; admin_cn: string }
  | { status: "allowed" }
  | { status: "denied"; reason: string | null }
  | { status: "timed_out" };

export interface PendingRequestDto {
  requester_cn: string;
  note: string | null;
  requested_at: string;
  received_secs_ago: number;
}

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
  visible_sessions: string[];
  join_flow: JoinFlowDto | null;
  pending_requests: PendingRequestDto[];
  /// Admin-side: list of admitted member CNs. Drives the per-member Kick /
  /// Ban buttons in MainScreen.
  admitted: string[];
  /// Member-side: set when a KickNotice landed for our own CN.
  kicked: KickedDto | null;
}
