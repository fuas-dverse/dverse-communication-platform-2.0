import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Props {
  prefillUsername?: string;
  onBack: (username: string) => void;
}

export default function RegisterScreen({ prefillUsername = "", onBack }: Props) {
  const [username, setUsername] = useState(prefillUsername);
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [working, setWorking] = useState(false);

  async function handleRegister() {
    if (working) return;
    setWorking(true);
    setError(null);
    setSuccess(null);
    try {
      const msg = await invoke<string>("register", {
        username, password, confirm,
      });
      setSuccess(msg);
    } catch (e) {
      setError(String(e));
    } finally {
      setWorking(false);
    }
  }

  return (
    <div className="flex flex-col items-center justify-center h-full">
      <div className="w-full max-w-sm space-y-6">
        <div className="text-center">
          <h1 className="text-2xl font-semibold text-gray-100">Create a dverse account</h1>
          <p className="text-sm text-gray-500 mt-1">
            Your account will be created on https://auth.dverse.yordanmitev.me
          </p>
        </div>

        <div className="space-y-4">
          <Field label="Username">
            <input
              type="text"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              className="input"
              placeholder="alice"
              autoFocus
            />
          </Field>

          <Field label="Password">
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              className="input"
            />
          </Field>

          <Field label="Confirm password">
            <input
              type="password"
              value={confirm}
              onChange={(e) => setConfirm(e.target.value)}
              className="input"
              onKeyDown={(e) => e.key === "Enter" && handleRegister()}
            />
          </Field>
        </div>

        {error && <p className="text-sm text-red-400">{error}</p>}
        {success && <p className="text-sm text-green-400">{success}</p>}

        <div className="flex gap-3">
          <button onClick={handleRegister} disabled={working} className="btn-primary flex-1">
            {working ? "Creating…" : "Create account"}
          </button>
          <button onClick={() => onBack(username)} className="btn-secondary">
            Back to sign in
          </button>
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
