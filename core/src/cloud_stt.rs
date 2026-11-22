//! Optional cloud speech-to-text providers (OpenAI, Google Cloud).
//!
//! Opt-in only — `Settings::stt_provider` defaults to `Local` (whisper.cpp).
//! Choosing a cloud provider sends the raw dictation audio to that
//! provider's servers; nothing here runs unless the user explicitly selects
//! it in Settings and supplies their own API key.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::time::Duration;

use crate::audio::TARGET_SAMPLE_RATE;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Encodes 16 kHz mono f32 samples as a 16-bit PCM WAV file in memory.
fn encode_wav_pcm16(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut buf = Vec::with_capacity(44 + data_len as usize);
    let channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    let byte_rate = TARGET_SAMPLE_RATE * channels as u32 * bits_per_sample as u32 / 8;
    let block_align = channels * bits_per_sample / 8;

    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_len).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&TARGET_SAMPLE_RATE.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let i16_sample = (clamped * i16::MAX as f32).round() as i16;
        buf.extend_from_slice(&i16_sample.to_le_bytes());
    }
    buf
}

/// Transcribes via the OpenAI audio transcription API
/// (`gpt-4o-transcribe`, `gpt-4o-mini-transcribe`, or `whisper-1`).
///
/// `language` is a whisper-style code ("en", "th", ...) or "auto" (omitted,
/// letting the model detect it). `prompt` biases decoding toward personal-
/// dictionary terms, same role as whisper's `initial_prompt`.
pub fn openai_transcribe(
    samples: &[f32],
    api_key: &str,
    model: &str,
    language: &str,
    prompt: &str,
) -> Result<String> {
    if api_key.trim().is_empty() {
        bail!("OpenAI API key is not set (Settings → Speech-to-text)");
    }
    let wav = encode_wav_pcm16(samples);

    let client = reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .context("build http client")?;

    let part = reqwest::blocking::multipart::Part::bytes(wav)
        .file_name("dictation.wav")
        .mime_str("audio/wav")
        .context("set mime type")?;
    let mut form = reqwest::blocking::multipart::Form::new()
        .part("file", part)
        .text("model", model.to_string());
    if language != "auto" && !language.is_empty() {
        form = form.text("language", language.to_string());
    }
    if !prompt.is_empty() {
        form = form.text("prompt", prompt.to_string());
    }

    let resp = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .context("request to OpenAI failed")?;

    let status = resp.status();
    let body = resp.text().context("read OpenAI response")?;
    if !status.is_success() {
        bail!("OpenAI STT error ({status}): {}", trim_error_body(&body));
    }

    #[derive(Deserialize)]
    struct OpenAiTranscription {
        text: String,
    }
    let parsed: OpenAiTranscription =
        serde_json::from_str(&body).context("parse OpenAI response")?;
    Ok(parsed.text.trim().to_string())
}

/// Transcribes via Google Cloud Speech-to-Text `v1/speech:recognize`
/// (synchronous recognize — suited to the short push-to-talk clips LocalFlow
/// records; Google caps sync requests at ~60s of audio).
///
/// `language` is mapped from whisper-style codes to Google's BCP-47 codes.
pub fn google_transcribe(
    samples: &[f32],
    api_key: &str,
    language: &str,
    phrases: &[String],
) -> Result<String> {
    if api_key.trim().is_empty() {
        bail!("Google Cloud API key is not set (Settings → Speech-to-text)");
    }
    let wav = encode_wav_pcm16(samples);
    let audio_b64 = base64_encode(&wav);
    let lang_code = google_language_code(language);

    let mut speech_contexts = serde_json::Value::Null;
    if !phrases.is_empty() {
        speech_contexts = serde_json::json!([{ "phrases": phrases }]);
    }

    let mut config = serde_json::json!({
        "encoding": "LINEAR16",
        "sampleRateHertz": TARGET_SAMPLE_RATE,
        "languageCode": lang_code,
        "enableAutomaticPunctuation": true,
    });
    if !speech_contexts.is_null() {
        config["speechContexts"] = speech_contexts;
    }

    let body = serde_json::json!({
        "config": config,
        "audio": { "content": audio_b64 },
    });

    let client = reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .context("build http client")?;

    let resp = client
        .post("https://speech.googleapis.com/v1/speech:recognize")
        .query(&[("key", api_key)])
        .json(&body)
        .send()
        .context("request to Google Cloud failed")?;

    let status = resp.status();
    let text = resp.text().context("read Google response")?;
    if !status.is_success() {
        bail!("Google STT error ({status}): {}", trim_error_body(&text));
    }

    #[derive(Deserialize)]
    struct Recognize {
        #[serde(default)]
        results: Vec<RecognizeResult>,
    }
    #[derive(Deserialize)]
    struct RecognizeResult {
        #[serde(default)]
        alternatives: Vec<Alternative>,
    }
    #[derive(Deserialize)]
    struct Alternative {
        #[serde(default)]
        transcript: String,
    }
    let parsed: Recognize = serde_json::from_str(&text).context("parse Google response")?;
    let joined = parsed
        .results
        .iter()
        .filter_map(|r| r.alternatives.first())
        .map(|a| a.transcript.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(joined.trim().to_string())
}

fn trim_error_body(body: &str) -> String {
    let t = body.trim();
    if t.len() > 300 {
        format!("{}…", &t[..300])
    } else {
        t.to_string()
    }
}

/// Maps a whisper-style language code to a Google Cloud BCP-47 locale.
/// Falls back to a best-effort guess for codes not in the picker list.
fn google_language_code(code: &str) -> String {
    match code {
        "en" => "en-US",
        "th" => "th-TH",
        "es" => "es-ES",
        "zh" => "zh-CN",
        "ja" => "ja-JP",
        "ko" => "ko-KR",
        "hi" => "hi-IN",
        "ar" => "ar-SA",
        "fr" => "fr-FR",
        "de" => "de-DE",
        "pt" => "pt-BR",
        "vi" => "vi-VN",
        "id" => "id-ID",
        // "auto" isn't supported by sync recognize; default to English rather
        // than fail outright.
        _ => "en-US",
    }
    .to_string()
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_is_well_formed() {
        let samples = vec![0.0f32, 0.5, -0.5, 1.0, -1.0];
        let wav = encode_wav_pcm16(&samples);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + samples.len() * 2);
    }

    #[test]
    fn google_language_code_maps_known_codes() {
        assert_eq!(google_language_code("th"), "th-TH");
        assert_eq!(google_language_code("auto"), "en-US");
        assert_eq!(google_language_code("xx"), "en-US");
    }
}
