import { useEffect, useRef, useState } from "react";
import type {
  AppConfig,
  HealthResponse,
  ModelEntry,
  StatusResponse,
} from "./types";
import {
  getConfig,
  getHealth,
  getModels,
  getStatus,
  initApiKey,
} from "./api";
import StatusGrid from "./components/StatusGrid";
import ModelList from "./components/ModelList";
import Settings from "./components/Settings";

type Tab = "status" | "models" | "settings";

const TABS: { id: Tab; label: string }[] = [
  { id: "status", label: "Status" },
  { id: "models", label: "Models" },
  { id: "settings", label: "Settings" },
];

export default function App() {
  const [tab, setTab] = useState<Tab>("status");
  const [status, setStatus] = useState<StatusResponse | null>(null);
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [models, setModels] = useState<ModelEntry[]>([]);
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [connected, setConnected] = useState(true);
  const [lastTick, setLastTick] = useState<string>("--:--:--");
  const polling = useRef(false);

  async function refresh() {
    if (polling.current) return;
    polling.current = true;
    try {
      const [h, s, m, c] = await Promise.all([
        getHealth(),
        getStatus(),
        getModels(),
        getConfig(),
      ]);
      setHealth(h);
      setStatus(s);
      setModels(m.data);
      setConfig(c);
      setConnected(true);
      setLastTick(new Date().toLocaleTimeString());
    } catch {
      setConnected(false);
    } finally {
      polling.current = false;
    }
  }

  useEffect(() => {
    let alive = true;
    (async () => {
      await initApiKey();
      if (!alive) return;
      await refresh();
    })();
    const id = setInterval(() => alive && refresh(), 3000);
    const onShow = () => alive && refresh();
    document.addEventListener("visibilitychange", onShow);
    return () => {
      alive = false;
      clearInterval(id);
      document.removeEventListener("visibilitychange", onShow);
    };
  }, []);

  return (
    <div className="h-full flex flex-col">
      <header className="flex items-center justify-between px-5 py-3 border-b border-white/5">
        <div className="flex items-center gap-2">
          <span className="text-lg font-semibold tracking-tight">Ferrydock</span>
          <span
            className={`text-[10px] px-2 py-0.5 rounded-full border ${
              connected
                ? "border-emerald-400/40 text-emerald-300"
                : "border-rose-400/40 text-rose-300"
            }`}
          >
            {connected ? "connected" : "offline"}
          </span>
        </div>
        <span className="text-xs text-white/40">updated {lastTick}</span>
      </header>

      <nav className="flex gap-1 px-5 py-3">
        <div className="glass rounded-full p-1 flex gap-1">
          {TABS.map((t) => (
            <button
              key={t.id}
              onClick={() => setTab(t.id)}
              className={`px-4 py-1.5 text-sm rounded-full transition ${
                tab === t.id
                  ? "bg-white/15 text-white"
                  : "text-white/60 hover:text-white"
              }`}
            >
              {t.label}
            </button>
          ))}
        </div>
      </nav>

      <main className="flex-1 overflow-auto px-5 pb-6">
        {!connected && (
          <div className="glass rounded-2xl p-4 text-sm text-rose-200 mb-4">
            Cannot reach the local gateway at 127.0.0.1:41980. Make sure
            Ferrydock is running.
          </div>
        )}
        {tab === "status" && <StatusGrid status={status} health={health} />}
        {tab === "models" && <ModelList models={models} />}
        {tab === "settings" && (
          <Settings config={config} onSaved={() => refresh()} />
        )}
      </main>
    </div>
  );
}
