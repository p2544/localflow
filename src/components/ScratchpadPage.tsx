import { useEffect, useRef, useState } from "react";
import { api, onPipelineEvent } from "../lib/api";

// Dictate into LocalFlow's own window — no external text field needed.
export default function ScratchpadPage() {
  const [text, setText] = useState("");
  const [recording, setRecording] = useState(false);
  const [status, setStatus] = useState("");
  const capture = useRef(false);

  useEffect(() => {
    const un = onPipelineEvent((ev) => {
      if (!capture.current) return;
      switch (ev.type) {
        case "transcribing":
          setStatus("Transcribing…");
          break;
        case "cleaning":
          setStatus("Polishing…");
          break;
        case "done":
          setText((t) => (t ? t + "\n" : "") + ev.final_text);
          setStatus("");
          capture.current = false;
          break;
        case "empty":
          setStatus("No speech detected");
          capture.current = false;
          break;
        case "error":
          setStatus(ev.message);
          capture.current = false;
          break;
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  return (
    <div>
      <h1>Scratchpad</h1>
      <div className="row" style={{ marginBottom: 10 }}>
        {recording ? (
          <button
            className="btn"
            onClick={() => {
              setRecording(false);
              capture.current = true;
              api.stopDictation();
            }}
          >
            ⏹ Stop
          </button>
        ) : (
          <button
            className="btn secondary"
            onClick={() => {
              setRecording(true);
              setStatus("Recording…");
              api.startDictation();
            }}
          >
            ● Record
          </button>
        )}
        <span className="empty-note">{status}</span>
        <span style={{ flex: 1 }} />
        <button
          className="btn secondary small"
          onClick={() => navigator.clipboard.writeText(text)}
          disabled={!text}
        >
          Copy all
        </button>
        <button className="btn danger small" onClick={() => setText("")} disabled={!text}>
          Clear
        </button>
      </div>
      <textarea
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder="Dictated text lands here. You can also type."
        style={{ minHeight: 320 }}
      />
    </div>
  );
}
