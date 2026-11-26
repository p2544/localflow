import { useEffect, useState } from "react";
import { api, type ChimeStyle, type Settings } from "../lib/api";
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
  onChange: (s: Settings) => void | Promise<void>;
}) {
  const [mics, setMics] = useState<string[]>([]);
  const [hotkeyError, setHotkeyError] = useState("");

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
            <span className="hint">Click Change, then press the key combo you want.</span>
          </label>
          <span>
            <HotkeyRecorder
              value={settings.hotkey}
              onCapture={(accel) => {
                setHotkeyError("");
                Promise.resolve(set("hotkey", accel)).catch((e: unknown) =>
                  setHotkeyError(String(e))
                );
              }}
            />
            {hotkeyError && (
              <div className="hint" style={{ color: "#f87171" }}>
                {hotkeyError}
              </div>
            )}
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
          <label>
            Sound effects
            <span className="hint">A soft cue when recording starts and stops.</span>
          </label>
          <input
            type="checkbox"
            checked={settings.sound_effects}
            onChange={(e) => set("sound_effects", e.target.checked)}
          />
        </div>
        {settings.sound_effects && (
          <div className="row">
            <label>Sound</label>
            <span style={{ display: "flex", gap: 8 }}>
              <select
                value={settings.sound_style}
                onChange={(e) => {
                  const style = e.target.value as ChimeStyle;
                  set("sound_style", style);
                  api.previewChime(style);
                }}
              >
                <option value="bubble">Bubble</option>
                <option value="soft_pop">Soft pop</option>
                <option value="tick">Tick</option>
              </select>
              <button className="btn secondary" onClick={() => api.previewChime(settings.sound_style)}>
                ▶ Preview
              </button>
            </span>
          </div>
        )}
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

      <h2>Speech-to-text</h2>
      <div className="card">
        <div className="row">
          <label>
            Engine
            <span className="hint">
              Local runs fully offline. A cloud provider sends your dictation audio to that
              provider's servers — only enable it if you've accepted that trade-off.
            </span>
          </label>
          <select
            value={settings.stt_provider}
            onChange={(e) => set("stt_provider", e.target.value as Settings["stt_provider"])}
          >
            <option value="local">Local (whisper.cpp, offline)</option>
            <option value="open_ai">OpenAI (cloud)</option>
            <option value="google">Google Cloud (cloud)</option>
          </select>
        </div>
        {settings.stt_provider === "open_ai" && (
          <>
            <div className="row">
              <label>
                OpenAI API key
                <span className="hint">
                  Stored locally in settings.json. Get one at platform.openai.com/api-keys.
                </span>
              </label>
              <input
                type="password"
                value={settings.openai_api_key}
                onChange={(e) => set("openai_api_key", e.target.value)}
                placeholder="sk-..."
              />
            </div>
            <div className="row">
              <label>Model</label>
              <select
                value={settings.openai_stt_model}
                onChange={(e) => set("openai_stt_model", e.target.value)}
              >
                <option value="gpt-4o-mini-transcribe">gpt-4o-mini-transcribe (cheap, fast)</option>
                <option value="gpt-4o-transcribe">gpt-4o-transcribe (higher accuracy)</option>
                <option value="whisper-1">whisper-1</option>
              </select>
            </div>
          </>
        )}
        {settings.stt_provider === "google" && (
          <div className="row">
            <label>
              Google Cloud API key
              <span className="hint">
                Stored locally in settings.json. Enable the Speech-to-Text API on a Google Cloud
                project and create an API key.
              </span>
            </label>
            <input
              type="password"
              value={settings.google_api_key}
              onChange={(e) => set("google_api_key", e.target.value)}
              placeholder="AIza..."
            />
          </div>
        )}
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

// Codes that are modifiers on their own — held while picking the real key,
// never the final key themselves. Mapped to the token `global-hotkey`'s
// accelerator parser expects (see src-tauri's tauri-plugin-global-shortcut).
const MODIFIER_CODES: Record<string, string> = {
  ControlLeft: "Control",
  ControlRight: "Control",
  ShiftLeft: "Shift",
  ShiftRight: "Shift",
  AltLeft: "Alt",
  AltRight: "Alt",
  MetaLeft: "Super",
  MetaRight: "Super",
};

const IS_MAC = /Mac|iPhone|iPad|iPod/.test(navigator.userAgent);

// Lets the user press the combo they want instead of typing Tauri
// accelerator syntax. `KeyboardEvent.code` matches that syntax's key names
// almost verbatim (KeyA, Digit1, Space, F9, ArrowUp, ...), so capture only
// has to translate the modifier flags.
function HotkeyRecorder({
  value,
  onCapture,
}: {
  value: string;
  onCapture: (accelerator: string) => void;
}) {
  const [recording, setRecording] = useState(false);
  const [liveMods, setLiveMods] = useState<string[]>([]);

  useEffect(() => {
    if (!recording) return;
    const held = new Set<string>();

    const stop = () => {
      setRecording(false);
      setLiveMods([]);
    };

    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.code === "Escape") {
        stop();
        return;
      }
      const modToken = MODIFIER_CODES[e.code];
      if (modToken) {
        held.add(modToken);
        setLiveMods([...held]);
        return;
      }
      const mods: string[] = [];
      if (IS_MAC) {
        if (e.metaKey) mods.push("CommandOrControl");
        if (e.ctrlKey) mods.push("Control");
      } else {
        if (e.ctrlKey) mods.push("CommandOrControl");
        if (e.metaKey) mods.push("Super");
      }
      if (e.altKey) mods.push("Alt");
      if (e.shiftKey) mods.push("Shift");
      onCapture([...mods, e.code].join("+"));
      stop();
    };

    const onKeyUp = (e: KeyboardEvent) => {
      const modToken = MODIFIER_CODES[e.code];
      if (modToken) {
        held.delete(modToken);
        setLiveMods([...held]);
      }
    };

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
    };
  }, [recording]);

  return (
    <span style={{ display: "flex", gap: 8, alignItems: "center" }}>
      <kbd>
        {recording ? (liveMods.length ? `${liveMods.join("+")}+…` : "press a key…") : value}
      </kbd>
      <button
        className="btn secondary small"
        onClick={() => setRecording((r) => !r)}
      >
        {recording ? "Cancel (Esc)" : "Change"}
      </button>
    </span>
  );
}
