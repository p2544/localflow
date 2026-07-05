import { useEffect, useState } from "react";
import { api, onPipelineEvent, type HistoryEntry } from "../lib/api";

export default function HistoryPage() {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [query, setQuery] = useState("");

  const refresh = (q = query) => api.historyList(q, 100).then(setEntries);

  useEffect(() => {
    refresh("");
    const un = onPipelineEvent((ev) => {
      if (ev.type === "done") refresh();
    });
    return () => {
      un.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div>
      <h1>History</h1>
      <div className="row" style={{ marginBottom: 10 }}>
        <input
          type="text"
          placeholder="Search dictations…"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            refresh(e.target.value);
          }}
          style={{ flex: 1 }}
        />
        <button
          className="btn danger small"
          onClick={() => api.historyClear().then(() => refresh())}
        >
          Clear all
        </button>
      </div>
      <div className="card">
        {entries.length === 0 && <div className="empty-note">No dictations yet.</div>}
        {entries.map((e) => (
          <div className="list-item" key={e.id}>
            <div className="text">
              <div className="final">{e.final_text}</div>
              {e.raw_text !== e.final_text && <div className="raw">raw: {e.raw_text}</div>}
              <div className="meta">
                {new Date(e.created_at * 1000).toLocaleString()}
                {e.app_name && ` · ${e.app_name}`}
                {e.language && ` · ${e.language}`} · {e.duration_ms} ms
              </div>
            </div>
            <div style={{ display: "flex", gap: 6, flexDirection: "column" }}>
              <button
                className="btn secondary small"
                onClick={() => navigator.clipboard.writeText(e.final_text)}
              >
                Copy
              </button>
              <button
                className="btn danger small"
                onClick={() => api.historyDelete(e.id).then(() => refresh())}
              >
                Delete
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
