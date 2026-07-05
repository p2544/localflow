//! localflow-core — headless engine for LocalFlow.
//!
//! Pipeline: audio capture → VAD trim → ASR (whisper.cpp) → LLM cleanup
//! (llama.cpp) → text injection into the focused field of the frontmost app.
//! Everything runs locally; no network access except explicit model downloads.

pub mod audio;
pub mod cleanup;
pub mod dictionary;
pub mod history;
pub mod inject;
pub mod models;
pub mod pipeline;
pub mod settings;
pub mod vad;

#[cfg(feature = "asr-whisper")]
pub mod asr;
#[cfg(feature = "llm-llama")]
pub mod llm;

pub use pipeline::{PipelineEvent, PipelineHandle, StageTimings};
pub use settings::Settings;
