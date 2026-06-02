import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { DiscoveredRouter } from "../types";

interface Props {
  prefillUsername?: string;
  initialError?: string | null;
  onNavigateRegister: () => void;
}

export default function LoginScreen({
  prefillUsername = "",
  initialError = null,
  onNavigateRegister,
}: Props) {
  const [username, setUsername] = useState(prefillUsername);
  const [password, setPassword] = useState("");
  const [createSession, setCreateSession] = useState(true);
  const [joinAdminCn, setJoinAdminCn] = useState("");
  const [error, setError] = useState<string | null>(initialError);
  const [working, setWorking] = useState(false);

  const [routers, setRouters] = useState<DiscoveredRouter[]>([]);
  const [scanning, setScanning] = useState(false);
  const [selectedRouter, setSelectedRouter] = useState<DiscoveredRouter | null>(null);

  // Auto-scan when switching to "Join session".
  useEffect(() => {
    if (!createSession) {
      handleScan();
    } else {
      setRouters([]);
      setSelectedRouter(null);
    }
  }, [createSession]);

  async function handleScan() {
    setScanning(true);
    setRouters([]);
    setSelectedRouter(null);
    try {
      const found = await invoke<DiscoveredRouter[]>("discover_routers");
      setRouters(found);
    } catch (_) {}
    setScanning(false);
  }

  function selectRouter(r: DiscoveredRouter) {
    setSelectedRouter(r);
    setJoinAdminCn(r.session || r.cn);
  }

  async function handleLogin() {
    if (working) return;
    setWorking(true);
    setError(null);
    try {
      await invoke("login", {
        username,
        password,
        createSession,
        joinAdminCn,
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setWorking(false);
    }
  }

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Enter") handleLogin();
  }

  return (
    <div className="flex flex-col items-center justify-center h-full">
      <div className="w-full max-w-sm space-y-5">
        <div className="text-center">
          <h1 className="text-2xl font-semibold text-gray-100">Sign in to dverse</h1>
          <p className="text-sm text-gray-500 mt-1">https://auth.dverse.yordanmitev.me</p>
        </div>

        <div className="space-y-4">
          <Field label="Username">
            <input
              type="text"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              onKeyDown={handleKeyDown}
              className="input"
              placeholder="you@dverse.yordanmitev.me"
              autoFocus
            />
          </Field>

          <Field label="Password">
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              onKeyDown={handleKeyDown}
              className="input"
            />
          </Field>
        </div>

        <div className="space-y-3">
          <div className="flex rounded-lg border border-gray-700 p-0.5 bg-gray-900">
            <button
              type="button"
              onClick={() => setCreateSession(true)}
              className={`flex-1 py-1.5 text-sm rounded-md transition-all duration-150 font-medium ${
                createSession
                  ? "bg-zenoh-600 text-white shadow-sm"
                  : "text-gray-400 hover:text-gray-200"
              }`}
            >
              Create session
            </button>
            <button
              type="button"
              onClick={() => setCreateSession(false)}
              className={`flex-1 py-1.5 text-sm rounded-md transition-all duration-150 font-medium ${
                !createSession
                  ? "bg-zenoh-600 text-white shadow-sm"
                  : "text-gray-400 hover:text-gray-200"
              }`}
            >
              Join session
            </button>
          </div>

          {!createSession && (
            <div className="space-y-3">
              <div className="space-y-1.5">
                <div className="flex items-center justify-between">
                  <span className="text-xs text-gray-500">Available sessions</span>
                  <button
                    onClick={handleScan}
                    disabled={scanning}
                    className="text-xs text-zenoh-400 hover:text-zenoh-300 transition-colors disabled:opacity-50"
                  >
                    {scanning ? "Scanning…" : "Rescan"}
                  </button>
                </div>

                {scanning ? (
                  <div className="text-xs text-gray-600 py-3 text-center border border-gray-800 rounded-lg">
                    Scanning local network…
                  </div>
                ) : routers.length === 0 ? (
                  <div className="text-xs text-gray-600 py-3 text-center border border-gray-800 rounded-lg">
                    No sessions found on local network
                  </div>
                ) : (
                  <div className="space-y-1.5">
                    {routers.map((r) => (
                      <button
                        key={r.zenoh_addr}
                        onClick={() => selectRouter(r)}
                        className={`w-full text-left px-3 py-2 rounded-lg border transition-colors ${
                          selectedRouter?.zenoh_addr === r.zenoh_addr
                            ? "border-zenoh-500 bg-zenoh-900/20"
                            : "border-gray-700 hover:border-gray-600"
                        }`}
                      >
                        <div className="text-sm font-medium text-gray-200">
                          {r.session || r.cn || r.name}
                        </div>
                        <div className="text-xs text-gray-500 font-mono mt-0.5">
                          {r.zenoh_addr}
                        </div>
                      </button>
                    ))}
                  </div>
                )}
              </div>

              <Field label="Admin username">
                <input
                  type="text"
                  value={joinAdminCn}
                  onChange={(e) => {
                    setJoinAdminCn(e.target.value);
                    setSelectedRouter(null);
                  }}
                  onKeyDown={handleKeyDown}
                  className="input"
                  placeholder="e.g. alice"
                />
              </Field>
            </div>
          )}
        </div>

        {error && <p className="text-sm text-red-400">{error}</p>}

        <div className="flex gap-3">
          <button onClick={handleLogin} disabled={working} className="btn-primary flex-1">
            {working ? "Signing in…" : "Sign in"}
          </button>
          <button onClick={onNavigateRegister} className="btn-secondary">
            Register
          </button>
        </div>

        <div className="border-t border-gray-800 pt-4 grid grid-cols-2 gap-x-4 gap-y-1 text-xs">
          <span className="text-gray-600">Identity provider</span>
          <span className="text-gray-500">https://auth.dverse.yordanmitev.me</span>
          <span className="text-gray-600">Certificate authority</span>
          <span className="text-gray-500">https://ca.dverse.yordanmitev.me:9000</span>
          <span className="text-gray-600">Router listens on</span>
          <span className="text-gray-500">tls/0.0.0.0:7447</span>
        </div>
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
