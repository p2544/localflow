//! Speech-to-text via whisper.cpp (whisper-rs bindings, GGUF/GGML models).

use anyhow::{Context, Result};
use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct Transcriber {
    ctx: WhisperContext,
    threads: i32,
}

impl Transcriber {
    pub fn load(model_path: &Path, threads: usize) -> Result<Self> {
        let mut ctx_params = WhisperContextParameters::default();
        // macOS: Metal must be compiled in (arm64 build requirement) but the
        // bundled whisper.cpp Metal kernels return empty output on M4 — run
        // whisper on CPU. NEON is comfortably realtime for small/turbo models.
        #[cfg(target_os = "macos")]
        ctx_params.use_gpu(false);
        let ctx = WhisperContext::new_with_params(
            model_path
                .to_str()
                .context("model path is not valid UTF-8")?,
            ctx_params,
        )
        .context("failed to load whisper model")?;
        Ok(Self {
            ctx,
            threads: threads as i32,
        })
    }

    /// Transcribes 16 kHz mono f32 samples.
    ///
    /// `language` is a whisper code ("en", "th", ...) or "auto".
    /// `initial_prompt` biases decoding toward personal-dictionary terms.
    pub fn transcribe(
        &self,
        samples: &[f32],
        language: &str,
        initial_prompt: &str,
    ) -> Result<String> {
        let mut state = self.ctx.create_state().context("create whisper state")?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(self.threads);
        params.set_translate(false);
        params.set_language(if language == "auto" { None } else { Some(language) });
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_no_context(true);
        params.set_single_segment(false);
        if !initial_prompt.is_empty() {
            params.set_initial_prompt(initial_prompt);
        }

        // whisper.cpp requires at least ~1s of audio; pad short clips.
        let min_len = crate::audio::TARGET_SAMPLE_RATE as usize + 1600;
        let padded;
        let audio = if samples.len() < min_len {
            padded = {
                let mut v = samples.to_vec();
                v.resize(min_len, 0.0);
                v
            };
            &padded[..]
        } else {
            samples
        };

        state.full(params, audio).context("whisper full() failed")?;

        let n = state.full_n_segments().context("segment count")?;
        let mut text = String::new();
        for i in 0..n {
            if let Ok(seg) = state.full_get_segment_text(i) {
                text.push_str(&seg);
            }
        }
        Ok(text.trim().to_string())
    }
}
