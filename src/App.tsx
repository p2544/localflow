import { useEffect, useState } from "react";
import { api, onNavigate, onPipelineEvent, type Settings, type StageTimings } from "./lib/api";
import SettingsPage from "./components/SettingsPage";
import ModelsPage from "./components/ModelsPage";
import HistoryPage from "./components/HistoryPage";
import DictionaryPage from "./components/DictionaryPage";
import ScratchpadPage from "./components/ScratchpadPage";
import Onboarding from "./components/Onboarding";

type Route = "settings" | "models" | "dictionary" | "history" | "scratchpad" | "debug";

const ROUTES: { id: Route; label: string }[] = [
  { id: "settings", label: "Settings" },
  { id: "models", label: "Models" },
  { id: "dictionary", label: "Dictionary" },
  { id: "history", label: "History" },
  { id: "scratchpad", label: "Scratchpad" },
  { id: "debug", label: "Latency" },
];

export default function App() {
  const [route, setRoute] = useState<Route>("settings");
  const [settings, setSettings] = useState<Settings | null>(null);
  const [lastTimings, setLastTimings] = useState<StageTimings | null>(null);

  useEffect(() => {
    api.getSettings().then(setSettings);
    const un1 = onNavigate((r) => setRoute(r as Route));
    const un2 = onPipelineEvent((ev) => {
      if (ev.type === "done") setLastTimings(ev.timings);
    });
    return () => {
      un1.then((f) => f());
      un2.then((f) => f());
    };
  }, []);

  if (!settings) return null;

  if (!settings.onboarding_done) {
    return (
      <Onboarding
        settings={settings}
        onDone={async (s) => {
          const next = { ...s, onboarding_done: true };
          await api.setSettings(next);
          setSettings(next);
        }}
      />
    );
  }

  const save = async (s: Settings) => {
    setSettings(s);
    await api.setSettings(s);
  };

  return (
    <div className="app">
      <nav className="sidebar">
        <div className="brand">LocalFlow</div>
        {ROUTES.map((r) => (
          <button
            key={r.id}
            className={route === r.id ? "active" : ""}
            onClick={() => setRoute(r.id)}
          >
            {r.label}
          </button>
        ))}
      </nav>
      <main className="content">
        {route === "settings" && <SettingsPage settings={settings} onChange={save} />}
        {route === "models" && <ModelsPage settings={settings} onChange={save} />}
        {route === "dictionary" && <DictionaryPage />}
        {route === "history" && <HistoryPage />}
        {route === "scratchpad" && <ScratchpadPage />}
        {route === "debug" && <DebugPage timings={lastTimings} />}
      </main>
    </div>
  );
}

function DebugPage({ timings }: { timings: StageTimings | null }) {
  return (
    <div>
      <h1>Latency</h1>
      <div className="card">
        {timings ? (
          <div className="timings">
            <span>Record <b>{timings.record_ms} ms</b></span>
            <span>VAD <b>{timings.vad_ms} ms</b></span>
            <span>ASR <b>{timings.asr_ms} ms</b></span>
            <span>LLM <b>{timings.llm_ms} ms</b></span>
            <span>Inject <b>{timings.inject_ms} ms</b></span>
            <span>Total (after release) <b>{timings.total_ms} ms</b></span>
          </div>
        ) : (
          <div className="empty-note">
            Dictate something — per-stage timings of the last run appear here.
            Target: ≤ 1500 ms total after key release.
          </div>
        )}
      </div>
    </div>
  );
}
