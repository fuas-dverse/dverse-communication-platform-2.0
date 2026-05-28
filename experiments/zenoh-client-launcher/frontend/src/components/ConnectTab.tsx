import { useState, useEffect } from "react";
import { Store } from "@tauri-apps/plugin-store";
import type { NetworkConfig, ConnectionStatus } from "../types";

const STATUS_LABEL: Record<ConnectionStatus, string> = {
  disconnected: "Not connected",
  pending: "Waiting for admin approval...",
  approved: "Connected",
  denied: "Access denied",
};

const STATUS_COLOR: Record<ConnectionStatus, string> = {
  disconnected: "text-gray-400",
  pending: "text-yellow-400",
  approved: "text-green-400",
  denied: "text-red-400",
};

const DEFAULT_CONFIG: NetworkConfig = {
  routerAddress: "tcp/localhost:7447",
  username: "",
  assignedDns: null,
  status: "disconnected",
};

export default function ConnectTab() {
  const [config, setConfig] = useState<NetworkConfig>(DEFAULT_CONFIG);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    (async () => {
      const store = await Store.load("launcher-config.json");
      const saved = await store.get<NetworkConfig>("network");
      if (saved) setConfig(saved);
    })();
  }, []);

  async function handleSave() {
    setSaving(true);
    try {
      const store = await Store.load("launcher-config.json");
      await store.set("network", config);
      await store.save();
    } finally {
      setSaving(false);
    }
  }

  async function handleRequestAccess() {
    // Sends presence announcement on Zenoh — admin sees it and approves/denies.
    // For now: update status to pending and save.
    // Full implementation: invoke Tauri command that calls zenoh publish.
    const updated = { ...config, status: "pending" as ConnectionStatus };
    setConfig(updated);
    const store = await Store.load("launcher-config.json");
    await store.set("network", updated);
    await store.save();
  }

  return (
    <div className="max-w-lg space-y-6">
      <div>
        <h2 className="text-lg font-semibold text-gray-100 mb-1">Network Connection</h2>
        <p className="text-sm text-gray-400">
          Connect to a Zenoh router. The admin must approve your request before you can use the network.
        </p>
      </div>

      <div className="space-y-4">
        <Field label="Zenoh Router Address" hint="e.g. tcp/192.168.1.10:7447">
          <input
            type="text"
            value={config.routerAddress}
            onChange={(e) => setConfig({ ...config, routerAddress: e.target.value })}
            className="input"
            placeholder="tcp/localhost:7447"
          />
        </Field>

        <Field label="Your Username" hint="Shown to the admin — used to assign your local DNS">
          <input
            type="text"
            value={config.username}
            onChange={(e) => setConfig({ ...config, username: e.target.value })}
            className="input"
            placeholder="alice"
          />
        </Field>
      </div>

      {/* Status badge */}
      <div className="flex items-center gap-3 p-4 bg-gray-900 rounded-lg border border-gray-800">
        <span className={`text-sm font-medium ${STATUS_COLOR[config.status]}`}>
          ● {STATUS_LABEL[config.status]}
        </span>
        {config.assignedDns && (
          <span className="ml-auto text-xs text-gray-400 font-mono bg-gray-800 px-2 py-1 rounded">
            {config.assignedDns}
          </span>
        )}
      </div>

      <div className="flex gap-3">
        <button
          onClick={handleRequestAccess}
          disabled={!config.routerAddress || !config.username || config.status === "pending"}
          className="btn-primary"
        >
          Request Access
        </button>
        <button onClick={handleSave} disabled={saving} className="btn-secondary">
          {saving ? "Saving..." : "Save"}
        </button>
        {config.status !== "disconnected" && (
          <button
            onClick={() => setConfig({ ...config, status: "disconnected", assignedDns: null })}
            className="btn-danger"
          >
            Disconnect
          </button>
        )}
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
