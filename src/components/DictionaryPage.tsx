import { useEffect, useState } from "react";
import { api } from "../lib/api";

export default function DictionaryPage() {
  const [words, setWords] = useState<string[]>([]);
  const [draft, setDraft] = useState("");

  useEffect(() => {
    api.dictionaryList().then(setWords);
  }, []);

  const add = async () => {
    const w = draft.trim();
    if (!w) return;
    setWords(await api.dictionaryAdd(w));
    setDraft("");
  };

  return (
    <div>
      <h1>Personal dictionary</h1>
      <div className="empty-note">
        Names, jargon, and product terms added here bias speech recognition and
        are never "corrected" by the cleanup AI.
      </div>
      <div className="row" style={{ marginBottom: 12 }}>
        <input
          type="text"
          placeholder="Add a word or name…"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
          style={{ flex: 1 }}
        />
        <button className="btn" onClick={add}>
          Add
        </button>
      </div>
      <div>
        {words.map((w) => (
          <span className="chip" key={w}>
            {w}
            <button onClick={() => api.dictionaryRemove(w).then(setWords)}>✕</button>
          </span>
        ))}
        {words.length === 0 && <div className="empty-note">Dictionary is empty.</div>}
      </div>
    </div>
  );
}
