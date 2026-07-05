import { useEffect, useState } from "react";
import { api, type Settings } from "../lib/api";
import { enable, disable } from "@tauri-apps/plugin-autostart";

// Whisper language codes surfaced in the picker; "auto" = detect once per clip.
const LANGUAGES: [string, string][] = [
  ["en", "English"],
  ["th", "ไทย (Thai)"],
  ["auto", "Auto-detect"],
  ["es", "Español"],
  ["zh", "中文"],
  ["ja", "日本語"],
  ["ko", "한국어"],
  ["hi", "हिन्दी"],
  ["ar", "العربية"],
  ["fr", "Français"],
  ["de", "Deutsch"],
  ["pt", "Português"],
  ["vi", "Tiếng Việt"],
  ["id", "Bahasa Indonesia"],
];

export default function SettingsPage({
  settings,
  onChange,
}: {
  settings: Settings;
  onChange: (s: Settings) => void;
}) {
  const [mics, setMics] = useState<string[]>([]);
  const [hotkeyDraft, setHotkeyDraft] = useState(settings.hotkey);

  useEffect(() => {
    api.listMics().then(setMics);
  }, []);

  const set = <K extends keyof Settings>(key: K, value: Settings[K]) =>
    onChange({ ...settings, [key]: value });

  return (
    <div>
      <h1>Settings</h1>

      <div className="card">
        <div className="row">
          <label>
            Hotkey
            <span className="hint">
              Tauri accelerator syntax, e.g. CommandOrControl+Shift+Space, F9
            </span>
          </label>
          <span>
            <input
              type="text"
              value={hotkeyDraft}
              onChange={(e) => setHotkeyDraft(e.target.value)}
              onBlur={() => hotkeyDraft.trim() && set("hotkey", hotkeyDraft.trim())}
            />
          </span>
        </div>
        <div className="row">
          <label>
            Mode
            <span className="hint">Push-to-talk: hold to record. Toggle: tap to start/stop.</span>
          </label>
          <select
            value={settings.hotkey_mode}
            onChange={(e) => set("hotkey_mode", e.target.value as Settings["hotkey_mode"])}
          >
            <option value="push_to_talk">Push-to-talk (hold)</option>
            <option value="toggle">Hands-free (toggle)</option>
          </select>
        </div>
        <div className="row">
          <label>Language</label>
          <select value={settings.language} onChange={(e) => set("language", e.target.value)}>
            {LANGUAGES.map(([code, label]) => (
              <option key={code} value={code}>
                {label}
              </option>
            ))}
          </select>
        </div>
        <div className="row">
          <label>Microphone</label>
          <select value={settings.mic_device} onChange={(e) => set("mic_device", e.target.value)}>
            <option value="">System default</option>
            {mics.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
        </div>
      </div>

      <h2>Output</h2>
      <div className="card">
        <div className="row">
          <label>
            AI cleanup
            <span className="hint">Filler removal, punctuation, self-corrections, lists. Off = raw transcript.</span>
          </label>
          <input
            type="checkbox"
            checked={settings.cleanup_enabled}
            onChange={(e) => set("cleanup_enabled", e.target.checked)}
          />
        </div>
        <div className="row">
          <label>
            Insert method
            <span className="hint">Paste is fastest; Type works in apps that block pasting.</span>
          </label>
          <select
            value={settings.output_mode}
            onChange={(e) => set("output_mode", e.target.value as Settings["output_mode"])}
          >
            <option value="paste">Paste (clipboard, restored after)</option>
            <option value="type">Type (simulated keystrokes)</option>
          </select>
        </div>
      </div>

      <h2>System</h2>
      <div className="card">
        <div className="row">
          <label>Launch at login</label>
          <input
            type="checkbox"
            checked={settings.launch_at_login}
            onChange={async (e) => {
              const on = e.target.checked;
              try {
                if (on) await enable();
                else await disable();
              } catch {
                /* autostart unsupported in dev */
              }
              set("launch_at_login", on);
            }}
          />
        </div>
        <div className="row">
          <label>
            Keep history
            <span className="hint">Stored only in a local SQLite file on this machine.</span>
          </label>
          <input
            type="checkbox"
            checked={settings.history_enabled}
            onChange={(e) => set("history_enabled", e.target.checked)}
          />
        </div>
        <div className="row">
          <label>
            Low-memory mode
            <span className="hint">Unload AI models after 5 min idle (slower next dictation).</span>
          </label>
          <input
            type="checkbox"
            checked={settings.low_memory_unload_secs > 0}
            onChange={(e) => set("low_memory_unload_secs", e.target.checked ? 300 : 0)}
          />
        </div>
      </div>

      <div className="card">
        <div className="row">
          <label>
            Test dictation
            <span className="hint">Or just hold <kbd>{settings.hotkey}</kbd> anywhere.</span>
          </label>
          <TestButtons />
        </div>
      </div>
    </div>
  );
}

function TestButtons() {
  const [recording, setRecording] = useState(false);
  return recording ? (
    <button
      className="btn"
      onClick={() => {
        api.stopDictation();
        setRecording(false);
      }}
    >
      Stop &amp; insert
    </button>
  ) : (
    <button
      className="btn secondary"
      onClick={() => {
        api.startDictation();
        setRecording(true);
      }}
    >
      Start recording
    </button>
  );
}
