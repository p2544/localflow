//! Orchestrates one dictation: record → VAD trim → ASR → cleanup → inject.
//!
//! Owns the warm model contexts. All engine work happens on a dedicated
//! worker thread; the caller (Tauri command layer) talks to it through
//! `PipelineHandle` and receives `PipelineEvent`s for the UI.

use crate::audio::Recorder;
use crate::cleanup;
use crate::inject::{self, InjectOutcome};
use crate::settings::Settings;
use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Receiver, Sender};
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Per-stage wall-clock timings for the debug panel (milliseconds).
#[derive(Debug, Clone, Default, Serialize)]
pub struct StageTimings {
    pub record_ms: u64,
    pub vad_ms: u64,
    pub asr_ms: u64,
    pub llm_ms: u64,
    pub inject_ms: u64,
    pub total_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PipelineEvent {
    RecordingStarted,
    /// Periodic mic level for the pill waveform, [0,1].
    Level { value: f32, elapsed_secs: f32 },
    RecordingStopped,
    Transcribing,
    Cleaning,
    Done {
        raw_text: String,
        final_text: String,
        outcome: InjectOutcome,
        timings: StageTimings,
        history_id: Option<i64>,
    },
    /// No speech detected in the recording.
    Empty,
    Error { message: String },
    ModelLoading { which: String },
    ModelReady { which: String },
}

enum Cmd {
    StartRecording,
    /// stop and run the full pipeline; `discard` = stop without transcribing.
    StopRecording { discard: bool },
    /// Transcribe+clean provided samples (scratchpad path returns text only).
    ProcessToText { samples: Vec<f32>, reply: Sender<Result<(String, String)>> },
    ReloadSettings(Settings),
    /// Drop warm model contexts (low-memory idle unload).
    UnloadModels,
    CleanText { raw: String, reply: Sender<Result<String>> },
    Shutdown,
}

#[derive(Clone)]
pub struct PipelineHandle {
    tx: Sender<Cmd>,
    pub events: Receiver<PipelineEvent>,
    recording: Arc<Mutex<bool>>,
}

impl PipelineHandle {
    pub fn spawn(settings: Settings) -> Self {
        let (tx, rx) = unbounded::<Cmd>();
        let (etx, erx) = unbounded::<PipelineEvent>();
        let recording = Arc::new(Mutex::new(false));
        let rec_flag = recording.clone();
        std::thread::Builder::new()
            .name("localflow-pipeline".into())
            .spawn(move || Worker::new(settings, etx, rec_flag).run(rx))
            .expect("spawn pipeline thread");
        Self { tx, events: erx, recording }
    }

    pub fn start_recording(&self) {
        let _ = self.tx.send(Cmd::StartRecording);
    }

    pub fn stop_recording(&self, discard: bool) {
        let _ = self.tx.send(Cmd::StopRecording { discard });
    }

    pub fn is_recording(&self) -> bool {
        *self.recording.lock()
    }

    pub fn reload_settings(&self, s: Settings) {
        let _ = self.tx.send(Cmd::ReloadSettings(s));
    }

    pub fn unload_models(&self) {
        let _ = self.tx.send(Cmd::UnloadModels);
    }

    /// Synchronous transcribe+clean of raw samples (scratchpad mode).
    pub fn process_to_text(&self, samples: Vec<f32>) -> Result<(String, String)> {
        let (rtx, rrx) = unbounded();
        self.tx
            .send(Cmd::ProcessToText { samples, reply: rtx })
            .map_err(|_| anyhow!("pipeline thread gone"))?;
        rrx.recv().map_err(|_| anyhow!("pipeline thread gone"))?
    }

    /// Synchronous cleanup of already-transcribed text (tests/tools).
    pub fn clean_text(&self, raw: String) -> Result<String> {
        let (rtx, rrx) = unbounded();
        self.tx
            .send(Cmd::CleanText { raw, reply: rtx })
            .map_err(|_| anyhow!("pipeline thread gone"))?;
        rrx.recv().map_err(|_| anyhow!("pipeline thread gone"))?
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(Cmd::Shutdown);
    }
}

struct Worker {
    settings: Settings,
    events: Sender<PipelineEvent>,
    recording_flag: Arc<Mutex<bool>>,
    recorder: Option<Recorder>,
    record_started: Option<Instant>,
    #[cfg(feature = "asr-whisper")]
    asr: Option<crate::asr::Transcriber>,
    #[cfg(feature = "llm-llama")]
    llm: Option<crate::llm::CleanupLlm>,
    last_used: Instant,
}

impl Worker {
    fn new(
        settings: Settings,
        events: Sender<PipelineEvent>,
        recording_flag: Arc<Mutex<bool>>,
    ) -> Self {
        Self {
            settings,
            events,
            recording_flag,
            recorder: None,
            record_started: None,
            #[cfg(feature = "asr-whisper")]
            asr: None,
            #[cfg(feature = "llm-llama")]
            llm: None,
            last_used: Instant::now(),
        }
    }

    fn emit(&self, ev: PipelineEvent) {
        let _ = self.events.send(ev);
    }

    fn run(mut self, rx: Receiver<Cmd>) {
        loop {
            // Poll with timeout while recording (to stream level events) and
            // to service the low-memory idle unload.
            let cmd = if self.recorder.is_some() {
                match rx.recv_timeout(Duration::from_millis(66)) {
                    Ok(c) => Some(c),
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                        if let (Some(r), Some(t0)) = (&self.recorder, self.record_started) {
                            self.emit(PipelineEvent::Level {
                                value: r.level(),
                                elapsed_secs: t0.elapsed().as_secs_f32(),
                            });
                        }
                        None
                    }
                    Err(_) => break,
                }
            } else {
                let idle = self.settings.low_memory_unload_secs;
                let timeout = if idle > 0 {
                    Duration::from_secs(10)
                } else {
                    Duration::from_secs(3600)
                };
                match rx.recv_timeout(timeout) {
                    Ok(c) => Some(c),
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                        if idle > 0 && self.last_used.elapsed().as_secs() > idle {
                            self.do_unload();
                        }
                        None
                    }
                    Err(_) => break,
                }
            };
            let Some(cmd) = cmd else { continue };
            match cmd {
                Cmd::StartRecording => self.start_recording(),
                Cmd::StopRecording { discard } => self.stop_recording(discard),
                Cmd::ProcessToText { samples, reply } => {
                    let _ = reply.send(self.transcribe_and_clean(&samples));
                }
                Cmd::CleanText { raw, reply } => {
                    let _ = reply.send(self.clean(&raw));
                }
                Cmd::ReloadSettings(s) => {
                    // Model file changes require a reload of the affected engine.
                    #[cfg(feature = "asr-whisper")]
                    if s.asr_model != self.settings.asr_model {
                        self.asr = None;
                    }
                    #[cfg(feature = "llm-llama")]
                    if s.llm_model != self.settings.llm_model {
                        self.llm = None;
                    }
                    self.settings = s;
                }
                Cmd::UnloadModels => self.do_unload(),
                Cmd::Shutdown => break,
            }
        }
    }

    fn do_unload(&mut self) {
        #[cfg(feature = "asr-whisper")]
        {
            self.asr = None;
        }
        #[cfg(feature = "llm-llama")]
        {
            self.llm = None;
        }
        tracing::info!("models unloaded (low-memory idle)");
    }

    fn start_recording(&mut self) {
        if self.recorder.is_some() {
            return;
        }
        match Recorder::start(&self.settings.mic_device) {
            Ok(r) => {
                self.recorder = Some(r);
                self.record_started = Some(Instant::now());
                *self.recording_flag.lock() = true;
                self.emit(PipelineEvent::RecordingStarted);
                // Warm models in parallel with the user speaking.
                self.ensure_models_quietly();
            }
            Err(e) => self.emit(PipelineEvent::Error {
                message: format!("microphone: {e:#}"),
            }),
        }
    }

    fn stop_recording(&mut self, discard: bool) {
        let Some(recorder) = self.recorder.take() else {
            return;
        };
        *self.recording_flag.lock() = false;
        let record_ms = self
            .record_started
            .take()
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0);
        self.emit(PipelineEvent::RecordingStopped);
        if discard {
            let _ = recorder.stop();
            return;
        }
        let total0 = Instant::now();
        let samples = match recorder.stop() {
            Ok(s) => s,
            Err(e) => {
                self.emit(PipelineEvent::Error { message: format!("audio: {e:#}") });
                return;
            }
        };

        let mut timings = StageTimings { record_ms, ..Default::default() };

        let t = Instant::now();
        let Some(trimmed) = crate::vad::trim_silence(&samples) else {
            self.emit(PipelineEvent::Empty);
            return;
        };
        timings.vad_ms = t.elapsed().as_millis() as u64;

        self.emit(PipelineEvent::Transcribing);
        let t = Instant::now();
        let raw = match self.transcribe(&trimmed) {
            Ok(r) => r,
            Err(e) => {
                self.emit(PipelineEvent::Error { message: format!("ASR: {e:#}") });
                return;
            }
        };
        timings.asr_ms = t.elapsed().as_millis() as u64;
        if raw.trim().is_empty() {
            self.emit(PipelineEvent::Empty);
            return;
        }

        self.emit(PipelineEvent::Cleaning);
        let t = Instant::now();
        let final_text = match self.clean(&raw) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("LLM cleanup failed, falling back to rules: {e:#}");
                cleanup::rule_based_cleanup(&raw)
            }
        };
        timings.llm_ms = t.elapsed().as_millis() as u64;

        let t = Instant::now();
        let app_name = inject::frontmost_app_name();
        let outcome = match inject::inject_text(&final_text, self.settings.output_mode) {
            Ok(o) => o,
            Err(e) => {
                self.emit(PipelineEvent::Error { message: format!("inject: {e:#}") });
                return;
            }
        };
        timings.inject_ms = t.elapsed().as_millis() as u64;
        timings.total_ms = total0.elapsed().as_millis() as u64;

        let history_id = if self.settings.history_enabled
            && outcome != InjectOutcome::RefusedSecureField
        {
            crate::history::open_db()
                .and_then(|c| {
                    crate::history::History::add(
                        &c,
                        &raw,
                        &final_text,
                        &self.settings.language,
                        &app_name,
                        timings.total_ms as i64,
                    )
                })
                .ok()
        } else {
            None
        };

        self.last_used = Instant::now();
        self.emit(PipelineEvent::Done {
            raw_text: raw,
            final_text,
            outcome,
            timings,
            history_id,
        });
    }

    fn transcribe_and_clean(&mut self, samples: &[f32]) -> Result<(String, String)> {
        let trimmed = crate::vad::trim_silence(samples).unwrap_or_else(|| samples.to_vec());
        let raw = self.transcribe(&trimmed)?;
        let cleaned = self
            .clean(&raw)
            .unwrap_or_else(|_| cleanup::rule_based_cleanup(&raw));
        Ok((raw, cleaned))
    }

    #[cfg(any(feature = "asr-whisper", feature = "llm-llama"))]
    fn dictionary_words(&self) -> Vec<String> {
        crate::history::open_db()
            .and_then(|c| crate::dictionary::Dictionary::all(&c))
            .unwrap_or_default()
    }

    fn ensure_models_quietly(&mut self) {
        #[cfg(feature = "asr-whisper")]
        if self.asr.is_none() {
            self.emit(PipelineEvent::ModelLoading { which: "asr".into() });
            match self.load_asr() {
                Ok(a) => {
                    self.asr = Some(a);
                    self.emit(PipelineEvent::ModelReady { which: "asr".into() });
                }
                Err(e) => tracing::warn!("ASR preload failed: {e:#}"),
            }
        }
        #[cfg(feature = "llm-llama")]
        if self.llm.is_none() && self.settings.cleanup_enabled {
            self.emit(PipelineEvent::ModelLoading { which: "llm".into() });
            match self.load_llm() {
                Ok(l) => {
                    self.llm = Some(l);
                    self.emit(PipelineEvent::ModelReady { which: "llm".into() });
                }
                Err(e) => tracing::warn!("LLM preload failed: {e:#}"),
            }
        }
    }

    #[cfg(feature = "asr-whisper")]
    fn load_asr(&self) -> Result<crate::asr::Transcriber> {
        let path = Settings::models_dir()?.join(&self.settings.asr_model);
        if !path.exists() {
            return Err(anyhow!(
                "ASR model not downloaded yet: {}",
                self.settings.asr_model
            ));
        }
        crate::asr::Transcriber::load(&path, self.settings.effective_threads())
    }

    #[cfg(feature = "llm-llama")]
    fn load_llm(&self) -> Result<crate::llm::CleanupLlm> {
        let path = Settings::models_dir()?.join(&self.settings.llm_model);
        if !path.exists() {
            return Err(anyhow!(
                "LLM model not downloaded yet: {}",
                self.settings.llm_model
            ));
        }
        crate::llm::CleanupLlm::load(&path, self.settings.effective_threads())
    }

    fn transcribe(&mut self, samples: &[f32]) -> Result<String> {
        #[cfg(feature = "asr-whisper")]
        {
            if self.asr.is_none() {
                self.asr = Some(self.load_asr()?);
            }
            let words = self.dictionary_words();
            let prompt = crate::dictionary::Dictionary::initial_prompt(&words);
            let mut text = String::new();
            // Chunk long clips at silence gaps to bound whisper memory/latency.
            for (_, chunk) in crate::vad::split_at_silences(samples, 25.0) {
                let part = self.asr.as_ref().unwrap().transcribe(
                    &chunk,
                    &self.settings.language,
                    &prompt,
                )?;
                if !text.is_empty() && !part.is_empty() {
                    text.push(' ');
                }
                text.push_str(&part);
            }
            Ok(text)
        }
        #[cfg(not(feature = "asr-whisper"))]
        {
            let _ = samples;
            Err(anyhow!("built without ASR support"))
        }
    }

    fn clean(&mut self, raw: &str) -> Result<String> {
        if !self.settings.cleanup_enabled || !cleanup::should_use_llm(raw) {
            return Ok(cleanup::rule_based_cleanup(raw));
        }
        #[cfg(feature = "llm-llama")]
        {
            if self.llm.is_none() {
                self.llm = Some(self.load_llm()?);
            }
            let words = self.dictionary_words();
            let cleaned = self.llm.as_ref().unwrap().clean(raw, &words)?;
            if cleaned.is_empty() {
                return Ok(cleanup::rule_based_cleanup(raw));
            }
            Ok(cleaned)
        }
        #[cfg(not(feature = "llm-llama"))]
        Ok(cleanup::rule_based_cleanup(raw))
    }
}
