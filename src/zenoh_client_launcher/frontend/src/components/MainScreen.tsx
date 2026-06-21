import type { AppSnapshot, AgentStatus, RouterStatus, NodeInfo } from "../types";
import PendingRequestsPanel from "./PendingRequestsPanel";
import AdmittedMembersPanel from "./AdmittedMembersPanel";

interface Props {
  snapshot: AppSnapshot;
}

export default function MainScreen({ snapshot }: Props) {
  const {
    router_status,
    session_id,
    session_role,
    connected_nodes,
    log,
    pending_requests,
    admitted,
  } = snapshot;
  const isAdmin = session_role.kind === "admin";
  // For admin role, session_id == operator_cn (see bot_framework::config).
  const selfCn = session_id;

  return (
    <div className="flex flex-col h-full">
      {/* Session badge */}
      {session_id && (
        <div className="px-4 py-1.5 bg-gray-900 border-b border-gray-800 flex items-center gap-2 text-sm">
          <span className="text-gray-500">Session:</span>
          {session_role.kind === "admin" ? (
            <span className="text-blue-300 font-medium">Admin · {session_id}</span>
          ) : (
            <span className="text-green-300 font-medium">
              Joined · admin:{" "}
              {(session_role as { kind: "client"; admin_cn: string }).admin_cn}
            </span>
          )}
        </div>
      )}

      {/* Router status bar */}
      <div className="px-4 py-1.5 bg-gray-900 border-b border-gray-800 flex items-center gap-2 text-sm">
        <span className="text-gray-500">Router:</span>
        <RouterStatusBadge status={router_status} />
      </div>

      {/* Main area */}
      <div className="flex-1 overflow-hidden flex">
        <div className="flex-1 overflow-hidden flex flex-col">
          {/* Connected nodes */}
          <div className="flex-1 overflow-y-auto p-4">
            <h2 className="text-sm font-semibold text-gray-300 mb-3">Connected nodes</h2>
            {connected_nodes.length === 0 ? (
              <p className="text-sm text-gray-600">Waiting for nodes to connect…</p>
            ) : (
              <div className="space-y-4">
                {connected_nodes.map((node) => (
                  <NodeCard key={node.cn} node={node} />
                ))}
              </div>
            )}
          </div>

          {/* Log panel */}
          <div className="h-36 border-t border-gray-800 flex flex-col shrink-0">
            <div className="px-4 py-1 text-xs font-medium text-gray-500 border-b border-gray-800 shrink-0">
              Log
            </div>
            <div className="flex-1 overflow-y-auto p-2 space-y-px">
              {log.map((line, i) => (
                <div key={i} className="font-mono text-xs text-gray-400 leading-5">
                  {line}
                </div>
              ))}
            </div>
          </div>
        </div>

        {/* Admin-only side panel — pending join requests on top, admitted
            members (with Kick/Ban) below. Both render `null` when empty so
            the panel auto-collapses on a clean session. */}
        {isAdmin &&
          (pending_requests.length > 0 || admitted.filter((cn) => cn !== selfCn).length > 0) && (
            <aside className="w-72 shrink-0 border-l border-gray-800 bg-gray-900/40 overflow-y-auto">
              <PendingRequestsPanel requests={pending_requests} />
              <AdmittedMembersPanel admitted={admitted} selfCn={selfCn} />
            </aside>
          )}
      </div>
    </div>
  );
}

function RouterStatusBadge({ status }: { status: RouterStatus }) {
  if (typeof status === "object" && "error" in status) {
    return <span className="text-red-400">Error: {status.error}</span>;
  }
  const s = status as string;
  const labels: Record<string, string> = {
    idle: "Idle",
    acquiring: "Acquiring cert…",
    starting: "Starting…",
    running: "Running",
    reloading: "Reloading ACL…",
  };
  const colors: Record<string, string> = {
    idle: "text-gray-400",
    acquiring: "text-yellow-400",
    starting: "text-yellow-400",
    running: "text-green-400",
    reloading: "text-yellow-400",
  };
  return <span className={colors[s] ?? "text-gray-400"}>{labels[s] ?? s}</span>;
}

function NodeCard({ node }: { node: NodeInfo }) {
  const agents = Object.entries(node.agents).sort(([a], [b]) => a.localeCompare(b));

  return (
    <div>
      <div className="text-sm font-semibold text-gray-200 mb-1">{node.cn}</div>
      {agents.length === 0 ? (
        <p className="text-xs text-gray-600 pl-3">(no agents reported)</p>
      ) : (
        <div className="space-y-2 pl-3">
          {agents.map(([name, ag]) => (
            <div key={name}>
              <div className="flex items-center gap-2">
                <AgentDot status={ag.status} />
                <span className="font-mono text-sm text-gray-200">{name}</span>
                <span className="text-xs text-gray-500">v{ag.version}</span>
                <span className="text-xs text-gray-600">·</span>
                <span className="text-xs text-gray-600">
                  {formatAgo(ag.last_seen_secs_ago)}
                </span>
              </div>
              {(ag.publishes.length > 0 || ag.subscribes.length > 0) && (
                <div className="pl-5 mt-0.5 space-y-0.5">
                  {ag.publishes.map((ke) => (
                    <div key={ke} className="font-mono text-xs text-gray-500">
                      → {ke}
                    </div>
                  ))}
                  {ag.subscribes.map((ke) => (
                    <div key={ke} className="font-mono text-xs text-gray-500">
                      ← {ke}
                    </div>
                  ))}
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function AgentDot({ status }: { status: AgentStatus }) {
  const colors: Record<AgentStatus, string> = {
    online: "text-green-400",
    degraded: "text-yellow-400",
    offline: "text-gray-500",
  };
  return <span className={colors[status]}>●</span>;
}

function formatAgo(secs: number): string {
  if (secs < 60) return `${secs}s ago`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s ago`;
  return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m ago`;
}
