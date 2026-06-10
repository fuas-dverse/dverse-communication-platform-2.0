import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Props {
  prefillUsername?: string;
  initialError?: string | null;
  onNavigateRegister: () => void;
}

/**
 * Credentials-only login. The session role (Admin vs Client) is picked in
 * the Chooser screen that follows — the backend stages the credentials and
 * flips the screen to "chooser" once this succeeds (issue #110).
 */
export default function LoginScreen({ prefillUsername = "", initialError = null, onNavigateRegister }: Props) {
  const [username, setUsername] = useState(prefillUsername);
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(initialError);
  const [working, setWorking] = useState(false);

  async function handleLogin() {
    if (working) return;
    setWorking(true);
    setError(null);
    try {
      await invoke("login", {
        payload: { username, password },
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
      <div className="w-full max-w-sm space-y-6">
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

        {error && <p className="text-sm text-red-400">{error}</p>}

        <div className="flex gap-3">
          <button
            onClick={handleLogin}
            disabled={working}
            className="btn-primary flex-1"
          >
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
