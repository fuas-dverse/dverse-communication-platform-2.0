import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppSnapshot } from "./types";
import LoginScreen from "./components/LoginScreen";
import RegisterScreen from "./components/RegisterScreen";
import ChooserScreen from "./components/ChooserScreen";
import RequestingJoinScreen from "./components/RequestingJoinScreen";
import LoadingScreen from "./components/LoadingScreen";
import MainScreen from "./components/MainScreen";
import ConnectTab from "./components/ConnectTab";
import BotsTab from "./components/BotsTab";

type MainTab = "nodes" | "network" | "bots";

export default function App() {
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [showRegister, setShowRegister] = useState(false);
  const [registerPrefill, setRegisterPrefill] = useState("");
  const [mainTab, setMainTab] = useState<MainTab>("nodes");

  const poll = useCallback(async () => {
    try {
      const s = await invoke<AppSnapshot>("get_state");
      setSnapshot(s);
      if (s.screen !== "login") setShowRegister(false);
    } catch (_) {}
  }, []);

  useEffect(() => {
    poll();
    const id = setInterval(poll, 500);
    return () => clearInterval(id);
  }, [poll]);

  if (!snapshot) {
    return (
      <div className="flex h-screen items-center justify-center bg-gray-950 text-gray-400 text-sm">
        Initialising…
      </div>
    );
  }

  if (showRegister) {
    return (
      <div className="flex h-screen bg-gray-950 p-6">
        <RegisterScreen
          prefillUsername={registerPrefill}
          onBack={(username) => {
            setShowRegister(false);
            setRegisterPrefill(username);
          }}
        />
      </div>
    );
  }

  if (snapshot.screen === "login") {
    return (
      <div className="flex h-screen bg-gray-950 p-6">
        <LoginScreen
          prefillUsername={registerPrefill}
          initialError={snapshot.error ?? null}
          onNavigateRegister={() => {
            setShowRegister(true);
            setRegisterPrefill("");
          }}
        />
      </div>
    );
  }

  if (snapshot.screen === "chooser") {
    return (
      <div className="flex h-screen bg-gray-950">
        <ChooserScreen />
      </div>
    );
  }

  if (snapshot.screen === "requesting_join") {
    return (
      <div className="flex h-screen bg-gray-950">
        <RequestingJoinScreen
          joinFlow={snapshot.join_flow}
          lastLog={snapshot.log.at(-1)}
        />
      </div>
    );
  }

  if (snapshot.screen === "loading") {
    return (
      <div className="flex h-screen bg-gray-950">
        <LoadingScreen lastLog={snapshot.log.at(-1)} />
      </div>
    );
  }

  return (
    <div className="flex flex-col h-screen bg-gray-950">
      <header className="flex items-center gap-4 px-6 py-3 bg-gray-900 border-b border-gray-800 shrink-0">
        <div className="flex items-center gap-2">
          <span className="text-zenoh-500 text-xl font-bold">⬡</span>
          <span className="font-semibold text-gray-100 tracking-tight">DVerse</span>
        </div>
        <nav className="flex gap-1 ml-4">
          {(["nodes", "network", "bots"] as MainTab[]).map((tab) => (
            <button
              key={tab}
              onClick={() => setMainTab(tab)}
              className={`px-4 py-1.5 rounded-md text-sm font-medium transition-colors ${
                mainTab === tab
                  ? "bg-zenoh-600 text-white"
                  : "text-gray-400 hover:text-gray-200 hover:bg-gray-800"
              }`}
            >
              {tab === "nodes" ? "Nodes" : tab === "network" ? "Network" : "My Bots"}
            </button>
          ))}
        </nav>
        <button
          onClick={() => invoke("logout")}
          className="ml-auto text-xs text-gray-500 hover:text-gray-300 transition-colors"
        >
          Sign out
        </button>
      </header>

      <div className="flex-1 overflow-hidden">
        {mainTab === "nodes" && <MainScreen snapshot={snapshot} />}
        {mainTab === "network" && (
          <div className="overflow-auto h-full p-6">
            <ConnectTab />
          </div>
        )}
        {mainTab === "bots" && (
          <div className="overflow-auto h-full p-6">
            <BotsTab />
          </div>
        )}
      </div>
    </div>
  );
}
