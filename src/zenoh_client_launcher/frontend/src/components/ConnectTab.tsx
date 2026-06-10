import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Store } from "@tauri-apps/plugin-store";
import type { NetworkConfig, ConnectionStatus, DiscoveredRouter } from "../types";

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
  zenohAddr: "",
  username: "",
  password: "",
  assignedDns: null,
  status: "disconnected",
};

export default function ConnectTab() {
  const [config, setConfig] = useState<NetworkConfig>(DEFAULT_CONFIG);
  const [routers, setRouters] = useState<DiscoveredRouter[]>([]);
  const [scanning, setScanning] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    (async () => {
      const store = await Store.load("launcher-config.json");
      const saved = await store.get<NetworkConfig>("network");
      if (saved) setConfig(saved);
    })();
  }, []);

  async function handleScan() {
    setScanning(true);
    setRouters([]);
    try {
      const found = await invoke<DiscoveredRouter[]>("discover_routers");
      setRouters(found);
      if (found.length === 0) {
        alert("No DVerse routers found on the local network.");
      }
    } catch (e) {
      alert(`Discovery failed: ${e}`);
    } finally {
      setScanning(false);
    }
  }

  function selectRouter(r: DiscoveredRouter) {
    setConfig((c) => ({ ...c, zenohAddr: r.zenoh_addr }));
  }

  async function handleRequestAccess() {
    const updated: NetworkConfig = { ...config, status: "pending" };
    setConfig(updated);
    const store = await Store.load("launcher-config.json");
    await store.set("network", updated);
    await store.save();
  }

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

  async function handleDisconnect() {
    const updated: NetworkConfig = { ...config, status: "disconnected", assignedDns: null };
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
          Scan for DVerse routers on your local network, pick one, and request access.
        </p>
      </div>

      {/* Router discovery */}
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium text-gray-300">Available Routers</span>
          <button onClick={handleScan} disabled={scanning} className="btn-secondary text-xs px-3 py-1.5">
            {scanning ? "Scanning..." : "Scan"}
          </button>
        </div>

        {routers.length > 0 ? (
          <div className="space-y-2">
            {routers.map((r) => (
              <button
                key={r.zenoh_addr}
                onClick={() => selectRouter(r)}
                className={`w-full text-left px-4 py-3 rounded-lg border transition-colors ${
                  config.zenohAddr === r.zenoh_addr
                    ? "border-zenoh-500 bg-zenoh-900/20 text-zenoh-200"
                    : "border-gray-700 hover:border-gray-600 text-gray-300"
                }`}
              >
                <div className="font-medium">{r.name}</div>
                <div className="text-xs text-gray-500 font-mono mt-0.5">{r.zenoh_addr}</div>
              </button>
            ))}
          </div>
        ) : (
          <div className="text-sm text-gray-600 py-3 text-center border border-gray-800 rounded-lg">
            {scanning ? "Scanning local network..." : "No routers discovered yet. Click Scan."}
          </div>
        )}

        {/* Manual override */}
        <div className="space-y-1">
          <label className="text-xs text-gray-500">Or enter address manually</label>
          <input
            type="text"
            value={config.zenohAddr}
            onChange={(e) => setConfig({ ...config, zenohAddr: e.target.value })}
            className="input text-sm"
            placeholder="tcp/192.168.1.10:7447"
          />
        </div>
      </div>

      {/* Credentials */}
      <div className="space-y-4">
        <Field label="Username">
          <input
            type="text"
            value={config.username}
            onChange={(e) => setConfig({ ...config, username: e.target.value })}
            className="input"
            placeholder="alice"
          />
        </Field>
        <Field label="Password">
          <input
            type="password"
            value={config.password}
            onChange={(e) => setConfig({ ...config, password: e.target.value })}
            className="input"
            placeholder="••••••••"
          />
        </Field>
      </div>

      {/* Status */}
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
          disabled={!config.zenohAddr || !config.username || config.status === "pending"}
          className="btn-primary"
        >
          Request Access
        </button>
        <button onClick={handleSave} disabled={saving} className="btn-secondary">
          {saving ? "Saving..." : "Save"}
        </button>
        {config.status !== "disconnected" && (
          <button onClick={handleDisconnect} className="btn-danger">
            Disconnect
          </button>
        )}
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1">
      <label className="text-sm font-medium text-gray-300">{label}</label>
      {children}
    </div>
  );
}
