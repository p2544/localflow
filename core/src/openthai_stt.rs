//! Optional local ASR engine: OpenThai2.0 ASR Streaming.
//!
//! A Thai-specialized streaming transducer (NVIDIA Nemotron family, by
//! OpenThaiGPT/iApp, OpenMDW-1.1 license) run via sherpa-onnx — a different
//! architecture from whisper.cpp, so it's a separate engine rather than just
//! another whisper model file. Fully offline once its model is downloaded
//! from the Models page; nothing here touches the network.
//!
//! LocalFlow doesn't do live partial-text display, so this feeds the whole
//! trimmed recording to the streaming recognizer in one shot (plus a short
//! tail pad to flush the last frames) and returns the final transcript,
//! mirroring how `transcribe_local` uses whisper.

use anyhow::{anyhow, Result};
use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig};
use std::path::Path;

use crate::audio::TARGET_SAMPLE_RATE;
use crate::models::openthai_files::{
    DECODER as DECODER_FILE, ENCODER as ENCODER_FILE, JOINER as JOINER_FILE,
    TOKENS as TOKENS_FILE,
};

/// feature_dim the published OpenThai2.0 sherpa-onnx export expects (80 is
/// sherpa-onnx's generic default, wrong for this model).
const FEATURE_DIM: i32 = 128;

pub fn is_installed(models_dir: &Path) -> bool {
    [ENCODER_FILE, DECODER_FILE, JOINER_FILE, TOKENS_FILE]
        .iter()
        .all(|f| models_dir.join(f).is_file())
}

pub struct OpenThaiAsr {
    recognizer: OnlineRecognizer,
}

impl OpenThaiAsr {
    pub fn load(models_dir: &Path, threads: usize) -> Result<Self> {
        if !is_installed(models_dir) {
            return Err(anyhow!(
                "OpenThai2.0 model not downloaded yet (Settings → Models)"
            ));
        }
        let path = |f: &str| models_dir.join(f).to_string_lossy().into_owned();

        let mut config = OnlineRecognizerConfig::default();
        config.model_config.transducer.encoder = Some(path(ENCODER_FILE));
        config.model_config.transducer.decoder = Some(path(DECODER_FILE));
        config.model_config.transducer.joiner = Some(path(JOINER_FILE));
        config.model_config.tokens = Some(path(TOKENS_FILE));
        config.model_config.provider = Some("cpu".to_string());
        config.model_config.num_threads = threads.max(1) as i32;
        config.feat_config.sample_rate = TARGET_SAMPLE_RATE as i32;
        config.feat_config.feature_dim = FEATURE_DIM;
        config.decoding_method = Some("greedy_search".to_string());
        config.enable_endpoint = false;

        let recognizer = OnlineRecognizer::create(&config)
            .ok_or_else(|| anyhow!("failed to load OpenThai2.0 ASR model (sherpa-onnx)"))?;
        Ok(Self { recognizer })
    }

    pub fn transcribe(&self, samples: &[f32]) -> Result<String> {
        let stream = self.recognizer.create_stream();
        let rate = TARGET_SAMPLE_RATE as i32;
        stream.accept_waveform(rate, samples);

        // ~0.3s of silence to flush the last frames through the transducer,
        // same as sherpa-onnx's own streaming examples.
        let tail_len = (TARGET_SAMPLE_RATE as f32 * 0.3).round() as usize;
        stream.accept_waveform(rate, &vec![0.0f32; tail_len]);
        stream.input_finished();

        let mut text = String::new();
        while self.recognizer.is_ready(&stream) {
            self.recognizer.decode(&stream);
            if let Some(result) = self.recognizer.get_result(&stream) {
                if !result.text.is_empty() {
                    text = result.text;
                }
            }
        }
        Ok(text.trim().to_string())
    }
}
