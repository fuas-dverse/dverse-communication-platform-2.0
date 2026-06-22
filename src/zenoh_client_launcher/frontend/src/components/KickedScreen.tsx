import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { KickedDto } from "../types";

interface Props {
  kicked: KickedDto;
}

/**
 * Member-side terminal screen (issue #111). Shown when this node received a
 * `KickNotice` addressed to its own CN. The session view has been torn down
 * by the backend (`kick_handler::handle_kick` clears `group_receivers` and
 * `member_olm_session`); this screen surfaces the admin's reason and a
 * "Back to chooser" action that calls `back_to_chooser` to reset the GUI.
 */
export default function KickedScreen({ kicked }: Props) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  async function back() {
    setBusy(true);
    setErr(null);
    try {
      await invoke("back_to_chooser");
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  const title = kicked.banned ? "You were banned" : "You were kicked";
  const subtitle = kicked.banned
    ? "The admin removed your node and blocked further join requests for the lifetime of their session."
    : "The admin removed your node from this session.";

  return (
    <div className="flex h-full bg-gray-950">
      <div className="m-auto w-full max-w-lg p-6 space-y-6 text-center">
        <div className="text-5xl">⛔</div>
        <h1 className="text-2xl font-semibold text-gray-100">{title}</h1>
        <p className="text-sm text-gray-400">{subtitle}</p>

        {kicked.reason && (
          <div className="px-4 py-3 rounded-md bg-gray-900 border border-gray-800 text-left">
            <div className="text-xs uppercase tracking-wide text-gray-500 mb-1">
              Reason
            </div>
            <div className="text-sm text-gray-200 break-words">
              {kicked.reason}
            </div>
          </div>
        )}

        {err && <p className="text-sm text-red-400">{err}</p>}

        <button
          onClick={back}
          disabled={busy}
          className="w-full px-4 py-2.5 rounded-lg border border-zenoh-500/50 bg-zenoh-500/10 hover:bg-zenoh-500/20 text-zenoh-200 text-sm font-medium transition-colors disabled:opacity-50"
        >
          {busy ? "Returning…" : "Back to chooser"}
        </button>
      </div>
    </div>
  );
}
