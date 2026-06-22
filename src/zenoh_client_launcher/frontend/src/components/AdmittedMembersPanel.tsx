import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Props {
  admitted: string[];
  /// The admin's own CN — excluded from the kick/ban targets so the admin
  /// can't kick themselves out of their own session.
  selfCn: string;
}

/**
 * Admin-side side panel (issue #111). Renders one row per admitted member CN
 * with Kick and Ban buttons. Lives below `PendingRequestsPanel` in the side
 * column of MainScreen and is admin-only (gated on `isAdmin` by MainScreen).
 *
 * - Kick: rotates the Megolm session, reshares the new SessionKey to other
 *   remaining members, publishes a KickNotice to the target, drops them from
 *   `admitted` and reloads the ACL. They can re-request to join.
 * - Ban: same as Kick PLUS the CN is added to the in-memory `banned_cns`
 *   set — their subsequent JoinRequests are silently dropped until the
 *   admin's session ends (or app restarts).
 */
export default function AdmittedMembersPanel({ admitted, selfCn }: Props) {
  const targets = admitted.filter((cn) => cn !== selfCn);
  if (targets.length === 0) return null;
  return (
    <div>
      <div className="px-4 py-2 border-b border-gray-800 text-xs font-medium text-gray-400">
        Admitted members
      </div>
      <ul className="p-3 space-y-3">
        {targets.map((cn) => (
          <MemberRow key={cn} cn={cn} />
        ))}
      </ul>
    </div>
  );
}

function MemberRow({ cn }: { cn: string }) {
  const [busy, setBusy] = useState<"" | "kick" | "ban">("");
  const [err, setErr] = useState<string | null>(null);
  const [reason, setReason] = useState<string>("");
  const [editing, setEditing] = useState(false);

  async function act(kind: "kick" | "ban") {
    setBusy(kind);
    setErr(null);
    try {
      const cmd = kind === "kick" ? "kick_member" : "ban_member";
      await invoke(cmd, {
        requesterCn: cn,
        reason: reason.trim().length === 0 ? null : reason.trim(),
      });
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy("");
    }
  }

  return (
    <li className="p-3 rounded-md bg-gray-900 border border-gray-800 space-y-2">
      <div className="text-sm font-medium text-gray-100 truncate">{cn}</div>
      {editing ? (
        <input
          type="text"
          value={reason}
          onChange={(e) => setReason(e.target.value)}
          placeholder="Optional reason"
          className="w-full px-2 py-1 text-xs rounded bg-gray-950 border border-gray-700 text-gray-200 placeholder-gray-600 focus:outline-none focus:border-gray-500"
        />
      ) : (
        <button
          onClick={() => setEditing(true)}
          className="text-xs text-gray-500 hover:text-gray-300 underline-offset-2 hover:underline"
        >
          {reason ? `Reason: "${reason}"` : "+ Add reason"}
        </button>
      )}
      {err && <div className="text-xs text-red-400">{err}</div>}
      <div className="flex gap-2 pt-1">
        <button
          onClick={() => act("kick")}
          disabled={busy !== ""}
          className="flex-1 px-2 py-1 text-xs rounded bg-yellow-600/20 text-yellow-200 border border-yellow-600/40 hover:bg-yellow-600/30 disabled:opacity-50"
        >
          {busy === "kick" ? "…" : "Kick"}
        </button>
        <button
          onClick={() => act("ban")}
          disabled={busy !== ""}
          className="flex-1 px-2 py-1 text-xs rounded bg-red-600/20 text-red-200 border border-red-600/40 hover:bg-red-600/30 disabled:opacity-50"
        >
          {busy === "ban" ? "…" : "Ban"}
        </button>
      </div>
    </li>
  );
}
