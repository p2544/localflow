//! Voice activity detection.
//!
//! Default engine: adaptive energy gate with hangover — dependency-free,
//! good enough for trimming leading/trailing silence in push-to-talk clips.
//! Optional (`vad-silero` feature): Silero VAD via ONNX Runtime for
//! model-based speech probability, used when the model file is present.

use crate::audio::TARGET_SAMPLE_RATE;

const FRAME_MS: usize = 30;
const FRAME_LEN: usize = TARGET_SAMPLE_RATE as usize * FRAME_MS / 1000;
/// Keep this much audio around detected speech edges.
const PAD_MS: usize = 250;
/// Frames of trailing non-speech tolerated inside a speech region.
const HANGOVER_FRAMES: usize = 12; // ~360ms

/// Trims leading/trailing silence; returns `None` when no speech at all.
pub fn trim_silence(samples: &[f32]) -> Option<Vec<f32>> {
    if samples.len() < FRAME_LEN * 2 {
        return None;
    }
    let energies: Vec<f32> = samples
        .chunks(FRAME_LEN)
        .map(|f| (f.iter().map(|s| s * s).sum::<f32>() / f.len() as f32).sqrt())
        .collect();

    // Adaptive threshold: noise floor estimated from the quietest 20% of frames.
    let mut sorted = energies.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let floor = sorted[sorted.len() / 5].max(1e-5);
    let peak = *sorted.last().unwrap();
    if peak < floor * 3.0 || peak < 0.005 {
        return None; // nothing louder than the noise floor
    }
    let threshold = (floor * 2.5).max(peak * 0.06).max(0.006);

    let speech: Vec<bool> = energies.iter().map(|&e| e > threshold).collect();
    let first = speech.iter().position(|&s| s)?;
    let last = speech.iter().rposition(|&s| s)?;

    // Reject blips shorter than ~90ms with nothing else around.
    let voiced = speech[first..=last].iter().filter(|&&s| s).count();
    if voiced < 3 {
        return None;
    }

    let pad = TARGET_SAMPLE_RATE as usize * PAD_MS / 1000;
    let start = (first * FRAME_LEN).saturating_sub(pad);
    let end = ((last + 1) * FRAME_LEN + pad).min(samples.len());
    Some(samples[start..end].to_vec())
}

/// Splits long audio into speech chunks at silence gaps so ASR can stream.
/// Returns (start_sample, chunk) pairs.
pub fn split_at_silences(samples: &[f32], max_chunk_secs: f32) -> Vec<(usize, Vec<f32>)> {
    let max_len = (max_chunk_secs * TARGET_SAMPLE_RATE as f32) as usize;
    if samples.len() <= max_len {
        return vec![(0, samples.to_vec())];
    }
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < samples.len() {
        let hard_end = (start + max_len).min(samples.len());
        let end = if hard_end == samples.len() {
            hard_end
        } else {
            // Search backwards from the hard cut for the quietest frame.
            let search_from = start + max_len / 2;
            let mut best = hard_end;
            let mut best_e = f32::MAX;
            let mut pos = search_from;
            while pos + FRAME_LEN < hard_end {
                let e: f32 = samples[pos..pos + FRAME_LEN].iter().map(|s| s * s).sum();
                if e < best_e {
                    best_e = e;
                    best = pos + FRAME_LEN / 2;
                }
                pos += FRAME_LEN;
            }
            best
        };
        chunks.push((start, samples[start..end].to_vec()));
        start = end;
    }
    let _ = HANGOVER_FRAMES; // reserved for the streaming VAD path
    chunks
}

#[cfg(feature = "vad-silero")]
pub mod silero {
    //! Silero VAD via ONNX Runtime. Loaded lazily from `models/silero_vad.onnx`.
    use anyhow::Result;
    use std::path::Path;

    pub struct SileroVad {
        session: ort::session::Session,
        state: Vec<f32>,
    }

    impl SileroVad {
        pub fn load(model_path: &Path) -> Result<Self> {
            let session = ort::session::Session::builder()?
                .commit_from_file(model_path)?;
            Ok(Self { session, state: vec![0.0; 2 * 1 * 128] })
        }

        /// Speech probability for one 512-sample 16 kHz frame.
        pub fn speech_prob(&mut self, frame: &[f32]) -> Result<f32> {
            use ort::value::Value;
            let input = Value::from_array(([1usize, frame.len()], frame.to_vec()))?;
            let state = Value::from_array(([2usize, 1, 128], self.state.clone()))?;
            let sr = Value::from_array(([1usize], vec![16000i64]))?;
            let outputs = self.session.run(ort::inputs![
                "input" => input,
                "state" => state,
                "sr" => sr,
            ])?;
            let prob = outputs[0].try_extract_tensor::<f32>()?.1[0];
            let (_, new_state) = outputs[1].try_extract_tensor::<f32>()?;
            self.state = new_state.to_vec();
            Ok(prob)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f32, amp: f32) -> Vec<f32> {
        (0..(secs * TARGET_SAMPLE_RATE as f32) as usize)
            .map(|i| (i as f32 * 0.08).sin() * amp)
            .collect()
    }

    #[test]
    fn trims_leading_and_trailing_silence() {
        let mut audio = vec![0.0005f32; TARGET_SAMPLE_RATE as usize]; // 1s near-silence
        audio.extend(tone(1.0, 0.4)); // 1s speech-loud tone
        audio.extend(vec![0.0005f32; TARGET_SAMPLE_RATE as usize]);
        let trimmed = trim_silence(&audio).expect("speech detected");
        let secs = trimmed.len() as f32 / TARGET_SAMPLE_RATE as f32;
        assert!(secs > 0.9 && secs < 1.8, "trimmed to {secs}s");
    }

    #[test]
    fn pure_silence_returns_none() {
        let audio = vec![0.0002f32; TARGET_SAMPLE_RATE as usize * 2];
        assert!(trim_silence(&audio).is_none());
    }

    #[test]
    fn splits_long_audio() {
        let audio = tone(25.0, 0.3);
        let chunks = split_at_silences(&audio, 10.0);
        assert!(chunks.len() >= 3);
        let total: usize = chunks.iter().map(|(_, c)| c.len()).sum();
        assert_eq!(total, audio.len());
    }
}
