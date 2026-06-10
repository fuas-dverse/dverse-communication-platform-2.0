import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { DiscoveredRouter } from "../types";

/**
 * Session chooser (issue #110). Replaces the old login create/join radio.
 * Lists sessions visible on the LAN via the existing `discover_routers`
 * mDNS browse, plus a "Create new session" button.
 *
 * Clicking a row → `pick_session({ admin_cn })` (Client role).
 * Clicking "Create new session" → `pick_session({ admin_cn: null })` (Admin role).
 * Either path stages the DverseConfig and boots the embedded router.
 */
export default function ChooserScreen() {
  const [sessions, setSessions] = useState<DiscoveredRouter[]>([]);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [picking, setPicking] = useState<string | null>(null);
  const [lastRefresh, setLastRefresh] = useState<number>(0);

  async function refresh() {
    if (refreshing) return;
    setRefreshing(true);
    setError(null);
    try {
      const found = await invoke<DiscoveredRouter[]>("discover_routers");
      setSessions(found);
      setLastRefresh(Date.now());
    } catch (e) {
      setError(String(e));
    } finally {
      setRefreshing(false);
    }
  }

  useEffect(() => {
    refresh();
    // No interval — the mDNS browse already has its own timeout; a
    // manual Refresh button is enough.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function pick(adminCn: string | null) {
    const label = adminCn ?? "new";
    setPicking(label);
    setError(null);
    try {
      await invoke("pick_session", { payload: { admin_cn: adminCn } });
    } catch (e) {
      setError(String(e));
      setPicking(null);
    }
  }

  return (
    <div className="flex h-full bg-gray-950">
      <div className="m-auto w-full max-w-xl p-6 space-y-6">
        <div className="text-center">
          <h1 className="text-2xl font-semibold text-gray-100">Choose a session</h1>
          <p className="text-sm text-gray-500 mt-1">
            Join a session visible on this network — or host your own.
          </p>
        </div>

        {/* Create new */}
        <button
          onClick={() => pick(null)}
          disabled={picking !== null}
          className="w-full px-4 py-3 rounded-lg border border-zenoh-500/50 bg-zenoh-500/10 hover:bg-zenoh-500/20 text-zenoh-200 text-sm font-medium transition-colors disabled:opacity-50"
        >
          {picking === "new" ? "Creating…" : "+ Create new session"}
        </button>

        <div className="border-t border-gray-800" />

        {/* Available sessions */}
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <h2 className="text-sm font-semibold text-gray-300">
              Sessions on this network
            </h2>
            <button
              onClick={refresh}
              disabled={refreshing}
              className="text-xs text-gray-500 hover:text-gray-300 disabled:opacity-50"
            >
              {refreshing ? "Refreshing…" : "↻ Refresh"}
            </button>
          </div>

          {sessions.length === 0 && !refreshing && (
            <p className="text-sm text-gray-600 text-center py-6">
              No sessions discovered yet.
              {lastRefresh > 0 && " mDNS scans take a few seconds."}
            </p>
          )}

          <ul className="space-y-2">
            {sessions.map((s) => {
              const adminCn = s.name;
              const busy = picking === adminCn;
              return (
                <li
                  key={`${s.name}-${s.zenoh_addr}`}
                  className="flex items-center justify-between gap-3 px-4 py-3 rounded-lg bg-gray-900 border border-gray-800 hover:border-gray-700 transition-colors"
                >
                  <div className="min-w-0">
                    <div className="text-sm font-medium text-gray-100 truncate">
                      {adminCn}
                    </div>
                    <div className="text-xs font-mono text-gray-500 truncate">
                      {s.zenoh_addr}
                    </div>
                  </div>
                  <button
                    onClick={() => pick(adminCn)}
                    disabled={picking !== null}
                    className="btn-secondary text-sm shrink-0 disabled:opacity-50"
                  >
                    {busy ? "Requesting…" : "Request to join"}
                  </button>
                </li>
              );
            })}
          </ul>
        </div>

        {error && <p className="text-sm text-red-400">{error}</p>}

        <div className="flex justify-end">
          <button
            onClick={() => invoke("logout")}
            className="text-xs text-gray-500 hover:text-gray-300"
          >
            Sign out
          </button>
        </div>
      </div>
    </div>
  );
}
