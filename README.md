# LocalFlow

> Repo: https://github.com/p2544/localflow — installers for Windows/macOS are built by CI on every push (Actions → latest run → Artifacts) and attached to Releases on version tags.

**Fully-local, system-wide voice dictation for Windows 11 and macOS 12+** — a privacy-first Wispr Flow alternative. Hold a hotkey anywhere, speak, release: AI-cleaned text appears in whatever text field has focus. No cloud, no account, no telemetry; the only network access is the explicit model download you trigger in the UI.

## How it works

```
hold hotkey ──► mic capture (cpal, 16 kHz mono)
                 └► VAD trim (energy gate; Silero optional)
                     └► ASR — whisper.cpp (GGUF, local)
                         └► LLM cleanup — llama.cpp (Qwen2.5-3B / Llama-3.2-3B, local)
                             fillers · punctuation · self-corrections · lists · numbers
                             └► inject into focused field
                                 macOS: AX API → ⌘V fallback | Windows: SendInput / Ctrl+V
```

- **Push-to-talk** (hold) or **hands-free toggle** mode
- **100+ languages** via Whisper (English + ไทย featured), selectable per session
- **Personal dictionary** biases recognition and is protected from LLM "correction"
- **History** (local SQLite, opt-out), **Scratchpad**, floating **recording pill**
- **Latency panel** shows per-stage ms (target ≤ 1.5 s after key release on consumer hardware)
- Password fields are refused; clipboard is saved/restored around paste injection

## Building

Prereqs: Rust (stable), Node 18+, and platform toolchains:

- **Windows**: Visual Studio Build Tools (C++), CMake. `npm i && npm run tauri build` → NSIS/MSI in `src-tauri/target/release/bundle/`.
- **macOS**: Xcode CLT, CMake (`brew install cmake`). `npm i && npm run tauri build` → .app/DMG. Grant *Microphone*, *Accessibility*, *Input Monitoring* on first run.
- **Linux (dev only)**: `sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libasound2-dev libssl-dev librsvg2-dev libayatana-appindicator3-dev pkg-config build-essential cmake` then `npm i && npm run tauri dev`. Text injection falls back to clipboard-copy on Linux.

Dev loop: `npm run tauri dev`. Rust tests: `cargo test --workspace --no-default-features` (fast, no models) — see below for the model-backed golden suite.

## Cleanup quality suite (M2)

30+ golden transcripts (EN + TH) covering fillers, backtracking ("5pm no wait 6pm" → "6pm"), spoken lists, number/email normalization, tone preservation:

```sh
# tier 1 — rules only, no model needed
cargo test -p localflow-core --no-default-features --test cleanup_suite

# tier 2 — real LLM (download a model first via the app, or any GGUF)
LOCALFLOW_LLM_MODEL=~/.local/share/LocalFlow/models/qwen2.5-3b-instruct-q4_k_m.gguf \
  cargo test -p localflow-core --test cleanup_suite -- --ignored --nocapture
```

## Models

Downloaded on demand from Hugging Face into the app data dir, sha256/size verified, resumable:

| Role | Default | Alternatives |
|---|---|---|
| ASR | Whisper Small (466 MB) | Base (fast) · Large-v3-Turbo Q5 (best, incl. Thai) |
| Cleanup LLM | Qwen2.5-3B-Instruct Q4 (~2 GB) | Llama-3.2-3B-Instruct Q4 |

Verify offline operation: disconnect the network after downloading — everything keeps working.

## Releases / installers

CI (`.github/workflows/build.yml`) builds Windows x64 NSIS+MSI and macOS universal DMG on every tag. Local equivalents: `npm run tauri build` on each OS.

## Repo layout

- `core/` — headless engine: audio, VAD, whisper, llama, cleanup rules, injection, SQLite, model manager
- `src-tauri/` — Tauri 2 shell: tray, global hotkey, pill window, IPC commands
- `src/` — React/TS UI: onboarding, settings, models, dictionary, history, scratchpad, latency panel
