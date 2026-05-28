import { useState } from "react";
import ConnectTab from "./components/ConnectTab";
import BotsTab from "./components/BotsTab";

type Tab = "connect" | "bots";

export default function App() {
  const [activeTab, setActiveTab] = useState<Tab>("connect");

  return (
    <div className="flex flex-col h-screen">
      {/* Header */}
      <header className="flex items-center gap-4 px-6 py-4 bg-gray-900 border-b border-gray-800">
        <div className="flex items-center gap-2">
          <span className="text-zenoh-500 text-xl font-bold">⬡</span>
          <span className="font-semibold text-gray-100 tracking-tight">Zenoh Client Launcher</span>
        </div>
        <nav className="flex gap-1 ml-6">
          {(["connect", "bots"] as Tab[]).map((tab) => (
            <button
              key={tab}
              onClick={() => setActiveTab(tab)}
              className={`px-4 py-1.5 rounded-md text-sm font-medium transition-colors ${
                activeTab === tab
                  ? "bg-zenoh-600 text-white"
                  : "text-gray-400 hover:text-gray-200 hover:bg-gray-800"
              }`}
            >
              {tab === "connect" ? "Network" : "My Bots"}
            </button>
          ))}
        </nav>
      </header>

      {/* Content */}
      <main className="flex-1 overflow-auto p-6">
        {activeTab === "connect" ? <ConnectTab /> : <BotsTab />}
      </main>
    </div>
  );
}
