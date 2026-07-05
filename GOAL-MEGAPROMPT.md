# /goal Megaprompt — "LocalFlow": a fully-local Wispr Flow clone for Windows 11 + macOS

> ใช้กับคำสั่ง `/goal` ได้เลย — วางเนื้อหาตั้งแต่หัวข้อ GOAL ลงไปเป็น argument
> สเปกนี้สังเคราะห์จาก deep research (2026-07-05) ของ Wispr Flow จริง + สถาปัตยกรรม open-source อ้างอิง (Handy, FluidVoice, OpenWhispr, VoiceTypr)

---

## GOAL

Build **LocalFlow** — a system-wide, privacy-first voice dictation desktop app for **Windows 11 (x64) and macOS 12+** that replicates Wispr Flow's core experience but runs **100% locally** (no cloud calls, no account, no telemetry). The user holds a global hotkey anywhere in the OS, speaks, releases, and AI-cleaned text appears in whatever text field currently has focus — in any application.

## PRODUCT SPEC (what Wispr Flow actually does — verified)

### Core loop (the whole product)
1. User presses/holds a **global hotkey** (default: hold `Ctrl+Win` on Windows / hold `Fn` or `⌥Space` on macOS — configurable). Two modes: **push-to-talk** (hold) and **hands-free toggle** (double-tap to latch, tap to stop).
2. A small **floating pill/overlay** appears near the cursor or screen-bottom showing live recording state (waveform animation, elapsed time).
3. Audio is captured from the default mic → **VAD** trims silence → **ASR** transcribes → **LLM post-processing** cleans the transcript → final text is **injected into the focused text field** of whatever app the user is in.
4. Target end-to-end latency after release-of-key: **≤ 1 second** on typical consumer hardware (Wispr's cloud target is ~700ms p99: <200ms ASR + <200ms LLM + network; local has no network hop but slower inference — treat 700ms as the design goal, 1.5s as acceptable ceiling).

### The AI post-processing layer (this is the differentiator — not raw STT)
The LLM cleanup step must, given the raw ASR transcript, produce edited text with:
- **Filler-word removal**: "um", "uh", "you know", "like" (when filler), stutters, repeated words.
- **Auto-punctuation & capitalization** inferred from sentence structure and pauses.
- **Backtracking / self-correction**: "meet at 5pm — no, actually 6pm" → "meet at 6pm". "Send it to John, I mean Jane" → "Send it to Jane".
- **Spoken list formatting**: "first... second... third..." or "one... two... three..." → a formatted numbered/bulleted list.
- **Number/date/email normalization**: "twenty five percent" → "25%", "john dot smith at gmail dot com" → "john.smith@gmail.com".
- **Tone preservation**: do NOT rewrite meaning or paraphrase; output must stay faithful — it's cleanup, not rewriting.
- **App-context awareness (v2, optional)**: detect the frontmost app name and adapt formatting (casual for chat apps, structured for editors/email). Never screenshot or upload anything.

### Supporting features
- **Personal dictionary**: user-defined words/names/jargon that bias ASR decoding (initial_prompt / hotword biasing) and are protected from LLM "correction". Auto-suggest additions from words the user re-types after dictation (v2).
- **Multi-language**: language picker (Whisper supports ~100 languages; expose at least English + Thai prominently). Per-session selection, NOT auto code-switching (Wispr doesn't truly do this either).
- **History panel**: local, searchable list of past dictations (raw + cleaned text), copy button, delete, and a "disable history" privacy toggle. Stored in local SQLite only.
- **Scratchpad/notes mode**: dictate into the app's own window when no external text field is focused.
- **Settings**: hotkey remap, mic device picker, language, model size picker (with download manager + disk-usage display), launch-at-login, output mode (paste vs keystroke simulation), LLM cleanup on/off (raw-transcript mode).
- **Onboarding**: first-run wizard that requests permissions (macOS: Microphone + Accessibility + Input Monitoring; Windows: mic), downloads the default models with progress UI, and runs a test dictation.

### Explicit non-goals (refuted or out of scope)
- No cloud processing, no accounts, no sync, no screenshots/URL tracking (Wispr Flow was criticized for its "context" screenshotting — we deliberately do the opposite).
- No automatic mid-sentence language switching (Wispr doesn't ship this either).
- No mobile apps. No per-app plugins — text injection is generic OS-level.

## TECHNICAL ARCHITECTURE (follow the proven open-source pattern)

### Stack: **Tauri 2 + Rust backend + React/TypeScript frontend** (the Handy architecture — proven cross-platform for exactly this app)
Single codebase, small binary, native performance for the audio/inference hot path. Alternative acceptable: Electron + native Node addons, but Tauri/Rust is preferred.

### Pipeline components (all local)
| Stage | Implementation |
|---|---|
| Global hotkey | `global-hotkey`/`rdev` crate (Win: low-level keyboard hook; macOS: CGEventTap — needs Input Monitoring permission) |
| Audio capture | `cpal` crate, 16kHz mono f32, ring buffer |
| VAD | **Silero VAD** (ONNX, via `ort` crate) — trim leading/trailing silence, chunk long dictations |
| ASR | **whisper.cpp** (via `whisper-rs`) with GGUF models. Default: `whisper-large-v3-turbo` quantized (~1.6GB, GPU via Metal on macOS / Vulkan or CUDA on Windows); fallback `small`/`base` for weak hardware. Optional alt engine: **NVIDIA Parakeet V3** via ONNX (CPU-optimized, ~5x realtime) — this is Handy's dual-engine approach. Pass personal dictionary via `initial_prompt`. |
| LLM cleanup | **llama.cpp** (via `llama-cpp-rs`) running a small instruct model (default: **Qwen2.5-3B-Instruct** or **Llama-3.2-3B-Instruct**, Q4 GGUF ~2GB) with a strict cleanup system prompt + few-shot examples covering fillers/backtracking/lists/numbers. Constrain: temperature 0, output = edited text only. Skip LLM entirely for transcripts <4 words (latency win). Wispr itself uses fine-tuned Llama for this step — same family, local. |
| Text injection — macOS | Primary: Accessibility API (`AXUIElement` set value/insert at focused element). Fallback: save clipboard → set clipboard → synthesize ⌘V via CGEvent → restore clipboard. Detect secure fields (password) and refuse. |
| Text injection — Windows | Primary: `SendInput` with `KEYEVENTF_UNICODE` (works everywhere incl. terminals). Fast path for long text: clipboard save → set → Ctrl+V → restore. Use UI Automation to verify a text control has focus. |
| Storage | SQLite (`rusqlite`) for history/dictionary/settings, in the platform app-data dir |
| Model manager | Download GGUF/ONNX models from Hugging Face with checksum + resume; store under app-data; let user pick sizes |

### Latency engineering
- Keep whisper + llama contexts **loaded and warm** in memory after first use (configurable "low-memory mode" unloads after idle).
- Start ASR **streaming during recording** (process rolling chunks) so only the tail remains at key-release.
- Run VAD in realtime on the capture stream.
- Measure and log per-stage timings (capture-stop → ASR done → LLM done → injected); show in a debug panel.

### Packaging & platform specifics
- **Windows**: MSI/NSIS installer via Tauri bundler, x64. Code-sign placeholder. Autostart via registry Run key (opt-in).
- **macOS**: .app + DMG, universal binary (arm64 + x86_64), hardened runtime + entitlements for mic; guide user through granting Accessibility + Input Monitoring in System Settings (deep-link to the panes). Login item via SMAppService.
- Menu-bar (macOS) / system-tray (Windows) resident app; main window only for settings/history/scratchpad.

## DELIVERABLES & MILESTONES
1. **M1 – Core loop CLI-quality**: hotkey → record → whisper.cpp transcribe → paste into focused field. Both OSes.
2. **M2 – LLM cleanup**: llama.cpp integration + cleanup prompt with test suite of ≥30 transcript→expected pairs (fillers, backtracking, lists, numbers, Thai + English).
3. **M3 – UI**: tray/menu-bar app, recording pill overlay, settings, onboarding + model downloader.
4. **M4 – Features**: personal dictionary, history panel, scratchpad, language picker, hands-free mode.
5. **M5 – Polish**: latency instrumentation, low-memory mode, installers for both OSes, README with build instructions.

## ACCEPTANCE CRITERIA
- Dictating a 2-sentence message into Notepad (Win) and Notes/Slack (macOS) produces correctly punctuated, filler-free text at the cursor, in ≤1.5s after key release on an M-series Mac / modern x64 PC with the default models.
- "Backtracking" test passes: speaking "let's meet at five pm no wait six pm" yields text containing "6pm" and not "5pm".
- List test passes: speaking "I need three things one apples two bananas three coffee" yields a 3-item formatted list.
- Personal-dictionary word (e.g., a Thai name or product codename) transcribes correctly after being added.
- Works with **zero network access** (verify with firewall/offline test).
- Password fields are never written to; clipboard is restored after paste-injection.

## REFERENCE IMPLEMENTATIONS (study before writing code)
- https://github.com/cjpais/Handy — Tauri+Rust+React, Silero VAD + Whisper GGUF/Parakeet V3, cross-platform, offline. Closest architectural template.
- https://github.com/altic-dev/FluidVoice — macOS-native "local Wispr Flow alternative" (on-device STT + local enhancement model).
- https://github.com/OpenWhispr/openwhispr — cross-platform, local Whisper + Parakeet + BYOK cloud option.
- https://github.com/moinulmoin/voicetypr — another OSS comparable.
- Wispr's own engineering notes (targets, not gospel): https://wisprflow.ai/post/technical-challenges , https://www.baseten.co/resources/customers/wispr-flow/
