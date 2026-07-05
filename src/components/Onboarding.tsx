import { useEffect, useState } from "react";
import {
  api,
  fmtBytes,
  onDownloadProgress,
  onPipelineEvent,
  type ModelSpec,
  type Settings,
} from "../lib/api";

const IS_MAC = navigator.userAgent.includes("Mac");

// First-run wizard: permissions → model downloads → test dictation.
export default function Onboarding({
  settings,
  onDone,
}: {
  settings: Settings;
  onDone: (s: Settings) => void;
}) {
  const [step, setStep] = useState(0);
  const [draft, setDraft] = useState(settings);
  const steps = [
    <Welcome key="w" onNext={() => setStep(1)} />,
    <Permissions key="p" onNext={() => setStep(2)} />,
    <Models key="m" draft={draft} setDraft={setDraft} onNext={() => setStep(3)} />,
    <TestRun key="t" draft={draft} onFinish={() => onDone(draft)} />,
  ];
  return <div className="onboarding">{steps[step]}</div>;
}

function Welcome({ onNext }: { onNext: () => void }) {
  return (
    <div className="card">
      <h1>Welcome to LocalFlow</h1>
      <p>
        Hold a hotkey anywhere, speak, release — clean text appears where your
        cursor is. Everything runs <b>100% on this computer</b>: no cloud, no
        account, no telemetry.
      </p>
      <p>Setup takes about two minutes (plus model download time).</p>
      <button className="btn" onClick={onNext}>
        Get started
      </button>
    </div>
  );
}

function Permissions({ onNext }: { onNext: () => void }) {
  return (
    <div className="card">
      <h1>Permissions</h1>
      <p>
        <span className="step-num">1</span>
        <b>Microphone</b> — you'll be prompted on first recording.
      </p>
      {IS_MAC && (
        <>
          <p>
            <span className="step-num">2</span>
            <b>Accessibility</b> — lets LocalFlow insert text into other apps.{" "}
            <button className="btn secondary small" onClick={() => api.openSettingsPane("accessibility")}>
              Open System Settings
            </button>
          </p>
          <p>
            <span className="step-num">3</span>
            <b>Input Monitoring</b> — lets the global hotkey work everywhere.{" "}
            <button className="btn secondary small" onClick={() => api.openSettingsPane("input-monitoring")}>
              Open System Settings
            </button>
          </p>
          <p className="empty-note">
            Enable LocalFlow in both panes, then come back here.
          </p>
        </>
      )}
      {!IS_MAC && (
        <p className="empty-note">
          On Windows no extra permissions are needed — the mic prompt appears on
          first use.
        </p>
      )}
      <button className="btn" onClick={onNext}>
        Continue
      </button>
    </div>
  );
}

function Models({
  draft,
  setDraft,
  onNext,
}: {
  draft: Settings;
  setDraft: (s: Settings) => void;
  onNext: () => void;
}) {
  const [catalog, setCatalog] = useState<ModelSpec[]>([]);
  const [installed, setInstalled] = useState<Set<string>>(new Set());
  const [downloading, setDownloading] = useState<string | null>(null);
  const [pct, setPct] = useState(0);
  const [error, setError] = useState("");

  const refresh = async () => {
    const st = await api.modelStatus();
    setInstalled(new Set(st.filter((s) => s.installed).map((s) => s.id)));
  };

  useEffect(() => {
    api.modelCatalog().then(setCatalog);
    refresh();
    const un = onDownloadProgress((p) => setPct(p.total ? p.downloaded / p.total : 0));
    return () => {
      un.then((f) => f());
    };
  }, []);

  // Recommended pair for first run.
  const asr = catalog.find((m) => m.id === "whisper-small");
  const llm = catalog.find((m) => m.id === "qwen2.5-3b-instruct");
  const asrDone = asr && installed.has(asr.id);
  const llmDone = llm && installed.has(llm.id);

  const grab = async (m: ModelSpec) => {
    setDownloading(m.id);
    setPct(0);
    setError("");
    try {
      await api.downloadModel(m.id);
      if (m.kind === "asr") setDraft({ ...draft, asr_model: m.file_name });
      else setDraft({ ...draft, llm_model: m.file_name });
    } catch (e) {
      setError(String(e));
    } finally {
      setDownloading(null);
      refresh();
    }
  };

  const row = (m: ModelSpec | undefined, done: boolean | undefined) =>
    m && (
      <div className="row">
        <label>
          {m.label}
          <span className="hint">{m.note}</span>
        </label>
        {done ? (
          <span style={{ color: "#4ade80" }}>✓ Installed</span>
        ) : downloading === m.id ? (
          <span className="progress" style={{ maxWidth: 200 }}>
            <div style={{ width: `${(pct * 100).toFixed(1)}%` }} />
          </span>
        ) : (
          <button className="btn small" disabled={!!downloading} onClick={() => grab(m)}>
            Download {fmtBytes(m.size_bytes)}
          </button>
        )}
      </div>
    );

  return (
    <div className="card">
      <h1>Download AI models</h1>
      <p className="empty-note">
        Both run entirely on-device. You can switch sizes later in Settings → Models.
      </p>
      {row(asr, asrDone)}
      {row(llm, llmDone)}
      {error && <p style={{ color: "#f87171" }}>{error}</p>}
      <div style={{ marginTop: 14, display: "flex", gap: 8 }}>
        <button className="btn" disabled={!asrDone} onClick={onNext}>
          Continue
        </button>
        {!llmDone && asrDone && (
          <span className="empty-note">
            (You can continue without the cleanup LLM — raw transcripts only.)
          </span>
        )}
      </div>
    </div>
  );
}

function TestRun({ draft, onFinish }: { draft: Settings; onFinish: () => void }) {
  const [recording, setRecording] = useState(false);
  const [result, setResult] = useState("");
  const [status, setStatus] = useState("");

  useEffect(() => {
    const un = onPipelineEvent((ev) => {
      if (ev.type === "transcribing") setStatus("Transcribing…");
      if (ev.type === "cleaning") setStatus("Polishing…");
      if (ev.type === "done") {
        setResult(ev.final_text);
        setStatus("");
      }
      if (ev.type === "empty") setStatus("No speech detected — try again.");
      if (ev.type === "error") setStatus(ev.message);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  return (
    <div className="card">
      <h1>Try it</h1>
      <p>
        Click record, say something like{" "}
        <i>"um so let's meet at five pm no wait six pm"</i>, then stop.
      </p>
      {recording ? (
        <button
          className="btn"
          onClick={() => {
            api.stopDictation();
            setRecording(false);
          }}
        >
          ⏹ Stop
        </button>
      ) : (
        <button
          className="btn secondary"
          onClick={() => {
            setResult("");
            setStatus("Recording…");
            api.startDictation();
            setRecording(true);
          }}
        >
          ● Record test
        </button>
      )}
      <p className="empty-note">{status}</p>
      {result && (
        <p>
          Result: <b>{result}</b>
        </p>
      )}
      <p className="empty-note">
        After setup, hold <kbd>{draft.hotkey}</kbd> in any app to dictate.
      </p>
      <button className="btn" onClick={onFinish}>
        Finish setup
      </button>
    </div>
  );
}
