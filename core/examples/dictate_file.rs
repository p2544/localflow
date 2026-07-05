//! Headless end-to-end check: WAV file → VAD → whisper → LLM cleanup.
//! Usage:
//!   cargo run -p localflow-core --example dictate_file --release -- \
//!       audio.wav [--language en] [--asr /path/ggml.bin] [--llm /path/model.gguf]
//! Prints per-stage timings against the ≤1.5s acceptance target.

use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let wav_path = args.first().expect("usage: dictate_file <wav> [--language xx]");
    let get = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let language = get("--language").unwrap_or_else(|| "en".into());
    let models_dir = localflow_core::settings::Settings::models_dir()?;
    let asr_path = get("--asr")
        .map(Into::into)
        .unwrap_or_else(|| models_dir.join("ggml-base.bin"));
    let llm_path = get("--llm")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| models_dir.join("qwen2.5-3b-instruct-q4_k_m.gguf"));

    // WAV → 16 kHz mono f32
    let mut reader = hound::WavReader::open(wav_path)?;
    let spec = reader.spec();
    let mono: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .filter_map(Result::ok)
                .collect::<Vec<_>>()
                .chunks(spec.channels as usize)
                .map(|f| f.iter().map(|s| *s as f32 / max).sum::<f32>() / f.len() as f32)
                .collect()
        }
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .filter_map(Result::ok)
            .collect::<Vec<_>>()
            .chunks(spec.channels as usize)
            .map(|f| f.iter().sum::<f32>() / f.len() as f32)
            .collect(),
    };
    let samples = localflow_core::audio::resample_to_16k(&mono, spec.sample_rate)?;
    println!(
        "audio: {:.2}s @ {} Hz -> {} samples @16k",
        mono.len() as f32 / spec.sample_rate as f32,
        spec.sample_rate,
        samples.len()
    );

    let t = Instant::now();
    let trimmed = localflow_core::vad::trim_silence(&samples).expect("no speech detected");
    println!("VAD: {:?} (kept {:.2}s)", t.elapsed(), trimmed.len() as f32 / 16000.0);

    let t = Instant::now();
    let asr = localflow_core::asr::Transcriber::load(&asr_path, 8)?;
    println!("ASR load: {:?}", t.elapsed());
    let t = Instant::now();
    let raw = asr.transcribe(&trimmed, &language, "")?;
    let asr_ms = t.elapsed();
    println!("ASR ({asr_ms:?}): {raw}");

    if llm_path.exists() {
        let t = Instant::now();
        let llm = localflow_core::llm::CleanupLlm::load(&llm_path, 8)?;
        println!("LLM load: {:?}", t.elapsed());
        let t = Instant::now();
        let cleaned = llm.clean(&raw, &[])?;
        println!("LLM ({:?}): {cleaned}", t.elapsed());
    } else {
        println!(
            "LLM skipped (no model at {}), rules: {}",
            llm_path.display(),
            localflow_core::cleanup::rule_based_cleanup(&raw)
        );
    }
    Ok(())
}
