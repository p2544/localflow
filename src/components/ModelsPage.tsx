import { useEffect, useState } from "react";
import {
  api,
  fmtBytes,
  onDownloadProgress,
  type ModelSpec,
  type ModelStatus,
  type Settings,
} from "../lib/api";

export default function ModelsPage({
  settings,
  onChange,
}: {
  settings: Settings;
  onChange: (s: Settings) => void;
}) {
  const [catalog, setCatalog] = useState<ModelSpec[]>([]);
  const [status, setStatus] = useState<Record<string, ModelStatus>>({});
  const [progress, setProgress] = useState<Record<string, number>>({});
  const [busy, setBusy] = useState<Record<string, boolean>>({});
  const [error, setError] = useState("");

  const refresh = async () => {
    const st = await api.modelStatus();
    setStatus(Object.fromEntries(st.map((s) => [s.id, s])));
  };

  useEffect(() => {
    api.modelCatalog().then(setCatalog);
    refresh();
    const un = onDownloadProgress((p) => {
      setProgress((prev) => ({ ...prev, [p.id]: p.total ? p.downloaded / p.total : 0 }));
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  const download = async (m: ModelSpec) => {
    setBusy((b) => ({ ...b, [m.id]: true }));
    setError("");
    try {
      await api.downloadModel(m.id);
      // Auto-select a freshly downloaded model if none of that kind is active.
      if (m.kind === "asr" && !status[activeAsrId(catalog, settings)]?.installed) {
        onChange({ ...settings, asr_model: m.file_name });
      }
      if (m.kind === "llm" && !status[activeLlmId(catalog, settings)]?.installed) {
        onChange({ ...settings, llm_model: m.file_name });
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy((b) => ({ ...b, [m.id]: false }));
      setProgress((p) => ({ ...p, [m.id]: 0 }));
      refresh();
    }
  };

  const section = (kind: "asr" | "llm", title: string, active: string, onSelect: (f: string) => void) => (
    <>
      <h2>{title}</h2>
      <div className="card">
        {catalog
          .filter((m) => m.kind === kind)
          .map((m) => {
            const st = status[m.id];
            const pct = progress[m.id] ?? 0;
            return (
              <div className="row" key={m.id}>
                <label>
                  <input
                    type="radio"
                    name={kind}
                    checked={active === m.file_name}
                    disabled={!st?.installed}
                    onChange={() => onSelect(m.file_name)}
                    style={{ marginRight: 8 }}
                  />
                  {m.label}
                  <span className="hint">{m.note}</span>
                </label>
                {st?.installed ? (
                  <button className="btn danger small" onClick={() => api.deleteModel(m.id).then(refresh)}>
                    Delete
                  </button>
                ) : busy[m.id] ? (
                  <span style={{ display: "flex", gap: 8, alignItems: "center", flex: "0 0 220px" }}>
                    <span className="progress">
                      <div style={{ width: `${(pct * 100).toFixed(1)}%` }} />
                    </span>
                    <button className="btn secondary small" onClick={() => api.cancelDownload(m.id)}>
                      ✕
                    </button>
                  </span>
                ) : (
                  <button className="btn small" onClick={() => download(m)}>
                    Download {fmtBytes(m.size_bytes)}
                  </button>
                )}
              </div>
            );
          })}
      </div>
    </>
  );

  return (
    <div>
      <h1>Models</h1>
      {error && <div className="card" style={{ color: "#f87171" }}>{error}</div>}
      {section("asr", "Speech recognition (Whisper)", settings.asr_model, (f) =>
        onChange({ ...settings, asr_model: f })
      )}
      {section("llm", "Cleanup LLM", settings.llm_model, (f) =>
        onChange({ ...settings, llm_model: f })
      )}
      <div className="empty-note">
        Models are stored locally and only downloaded when you click Download.
        Nothing else ever touches the network.
      </div>
    </div>
  );
}

function activeAsrId(catalog: ModelSpec[], s: Settings): string {
  return catalog.find((m) => m.file_name === s.asr_model)?.id ?? "";
}
function activeLlmId(catalog: ModelSpec[], s: Settings): string {
  return catalog.find((m) => m.file_name === s.llm_model)?.id ?? "";
}
