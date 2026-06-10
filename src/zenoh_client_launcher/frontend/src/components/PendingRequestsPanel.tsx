import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { PendingRequestDto } from "../types";

interface Props {
  requests: PendingRequestDto[];
}

/**
 * Admin-side side panel rendered inside MainScreen when there are pending
 * join requests. Each row shows the requester's CN + optional note and an
 * Allow / Deny pair that calls into `admit_request` / `deny_request`.
 */
export default function PendingRequestsPanel({ requests }: Props) {
  if (requests.length === 0) return null;
  return (
    <aside className="w-72 shrink-0 border-l border-gray-800 bg-gray-900/40 overflow-y-auto">
      <div className="px-4 py-2 border-b border-gray-800 text-xs font-medium text-gray-400">
        Pending join requests
      </div>
      <ul className="p-3 space-y-3">
        {requests.map((r) => (
          <RequestRow key={r.requester_cn} req={r} />
        ))}
      </ul>
    </aside>
  );
}

function RequestRow({ req }: { req: PendingRequestDto }) {
  const [busy, setBusy] = useState<"" | "allow" | "deny">("");
  const [err, setErr] = useState<string | null>(null);

  async function allow() {
    setBusy("allow");
    setErr(null);
    try {
      await invoke("admit_request", { requesterCn: req.requester_cn });
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy("");
    }
  }
  async function deny() {
    setBusy("deny");
    setErr(null);
    try {
      await invoke("deny_request", {
        requesterCn: req.requester_cn,
        reason: null,
      });
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy("");
    }
  }

  return (
    <li className="p-3 rounded-md bg-gray-900 border border-gray-800 space-y-2">
      <div className="text-sm font-medium text-gray-100 truncate">
        {req.requester_cn}
      </div>
      {req.note && (
        <div className="text-xs text-gray-400 italic break-words">
          "{req.note}"
        </div>
      )}
      <div className="text-xs text-gray-600">
        {req.received_secs_ago}s ago
      </div>
      {err && <div className="text-xs text-red-400">{err}</div>}
      <div className="flex gap-2 pt-1">
        <button
          onClick={allow}
          disabled={busy !== ""}
          className="flex-1 px-2 py-1 text-xs rounded bg-green-600/20 text-green-200 border border-green-600/40 hover:bg-green-600/30 disabled:opacity-50"
        >
          {busy === "allow" ? "…" : "Allow"}
        </button>
        <button
          onClick={deny}
          disabled={busy !== ""}
          className="flex-1 px-2 py-1 text-xs rounded bg-red-600/20 text-red-200 border border-red-600/40 hover:bg-red-600/30 disabled:opacity-50"
        >
          {busy === "deny" ? "…" : "Deny"}
        </button>
      </div>
    </li>
  );
}
