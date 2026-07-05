//! Model catalog + download manager (Hugging Face, sha256, resume).
//! The ONLY code in the app allowed to touch the network, and only when the
//! user explicitly requests a download.

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct ModelSpec {
    pub id: &'static str,
    pub kind: &'static str, // "asr" | "llm"
    pub file_name: &'static str,
    pub url: &'static str,
    /// Hex sha256; empty = skip verification (size check only).
    pub sha256: &'static str,
    pub size_bytes: u64,
    pub label: &'static str,
    pub note: &'static str,
}

/// Curated catalog. Sizes are approximate for progress UI; hash empty where
/// upstream repos don't publish stable hashes (verified by size instead).
pub const CATALOG: &[ModelSpec] = &[
    ModelSpec {
        id: "whisper-base",
        kind: "asr",
        file_name: "ggml-base.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        size_bytes: 147_951_465,
        label: "Whisper Base (fast, 142 MB)",
        note: "Good accuracy for clear speech; weakest on Thai.",
    },
    ModelSpec {
        id: "whisper-small",
        kind: "asr",
        file_name: "ggml-small.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        size_bytes: 487_601_967,
        label: "Whisper Small (balanced, 466 MB)",
        note: "Recommended minimum for Thai.",
    },
    ModelSpec {
        id: "whisper-large-v3-turbo",
        kind: "asr",
        file_name: "ggml-large-v3-turbo-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        size_bytes: 574_041_195,
        label: "Whisper Large v3 Turbo Q5 (best, 547 MB)",
        note: "Best accuracy incl. Thai; needs a decent CPU/GPU.",
    },
    ModelSpec {
        id: "qwen2.5-3b-instruct",
        kind: "llm",
        file_name: "qwen2.5-3b-instruct-q4_k_m.gguf",
        url: "https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf",
        sha256: "",
        size_bytes: 2_100_000_000,
        label: "Qwen2.5 3B Instruct Q4 (cleanup LLM, ~2 GB)",
        note: "Default cleanup model; strong multilingual incl. Thai.",
    },
    ModelSpec {
        id: "llama-3.2-3b-instruct",
        kind: "llm",
        file_name: "Llama-3.2-3B-Instruct-Q4_K_M.gguf",
        url: "https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF/resolve/main/Llama-3.2-3B-Instruct-Q4_K_M.gguf",
        sha256: "",
        size_bytes: 2_020_000_000,
        label: "Llama 3.2 3B Instruct Q4 (~1.9 GB)",
        note: "Alternative cleanup model (same family Wispr fine-tunes).",
    },
];

pub fn spec_by_id(id: &str) -> Option<&'static ModelSpec> {
    CATALOG.iter().find(|m| m.id == id)
}

pub fn spec_by_file(file_name: &str) -> Option<&'static ModelSpec> {
    CATALOG.iter().find(|m| m.file_name == file_name)
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    pub id: String,
    pub installed: bool,
    pub bytes_on_disk: u64,
}

pub fn status(models_dir: &Path) -> Vec<ModelStatus> {
    CATALOG
        .iter()
        .map(|m| {
            let p = models_dir.join(m.file_name);
            let bytes = std::fs::metadata(&p).map(|md| md.len()).unwrap_or(0);
            ModelStatus {
                id: m.id.to_string(),
                // "installed" = full size present (partial files use .part).
                installed: bytes > 0,
                bytes_on_disk: bytes,
            }
        })
        .collect()
}

/// Blocking download with resume + progress callback (downloaded, total).
/// Writes to `<file>.part`, verifies, then renames — a crash never leaves a
/// half-file that looks installed.
pub fn download(
    spec: &ModelSpec,
    models_dir: &Path,
    mut progress: impl FnMut(u64, u64),
    cancelled: &dyn Fn() -> bool,
) -> Result<PathBuf> {
    let final_path = models_dir.join(spec.file_name);
    if final_path.exists() {
        return Ok(final_path);
    }
    let part_path = models_dir.join(format!("{}.part", spec.file_name));
    let existing = std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let mut req = client.get(spec.url);
    if existing > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={existing}-"));
    }
    let resp = req.send().context("download request failed")?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("download failed: HTTP {status}"));
    }
    let resuming = status == reqwest::StatusCode::PARTIAL_CONTENT;
    let total = if resuming {
        existing + resp.content_length().unwrap_or(0)
    } else {
        resp.content_length().unwrap_or(spec.size_bytes)
    };

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&part_path)?;
    let mut written = if resuming {
        file.seek(SeekFrom::End(0))?;
        existing
    } else {
        file.set_len(0)?;
        0
    };

    let mut reader = resp;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancelled() {
            return Err(anyhow!("download cancelled"));
        }
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        written += n as u64;
        progress(written, total);
    }
    file.flush()?;
    drop(file);

    verify(&part_path, spec)?;
    std::fs::rename(&part_path, &final_path)?;
    Ok(final_path)
}

fn verify(path: &Path, spec: &ModelSpec) -> Result<()> {
    let md = std::fs::metadata(path)?;
    if !spec.sha256.is_empty() {
        let mut hasher = Sha256::new();
        let mut f = std::fs::File::open(path)?;
        std::io::copy(&mut f, &mut hasher)?;
        let got = hex::encode(hasher.finalize());
        if got != spec.sha256 {
            std::fs::remove_file(path).ok();
            return Err(anyhow!("sha256 mismatch: expected {}, got {got}", spec.sha256));
        }
    } else if md.len() < spec.size_bytes / 2 {
        std::fs::remove_file(path).ok();
        return Err(anyhow!("downloaded file suspiciously small ({} bytes)", md.len()));
    }
    Ok(())
}

pub fn delete_model(spec: &ModelSpec, models_dir: &Path) -> Result<()> {
    for name in [spec.file_name.to_string(), format!("{}.part", spec.file_name)] {
        let p = models_dir.join(name);
        if p.exists() {
            std::fs::remove_file(p)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_lookup() {
        assert!(spec_by_id("whisper-base").is_some());
        assert!(spec_by_file("ggml-base.bin").is_some());
        assert!(spec_by_id("nope").is_none());
    }

    #[test]
    fn status_reports_missing_models() {
        let dir = tempfile::tempdir().unwrap();
        let st = status(dir.path());
        assert_eq!(st.len(), CATALOG.len());
        assert!(st.iter().all(|s| !s.installed));
    }

    #[test]
    fn verify_rejects_tiny_file_without_hash() {
        let dir = tempfile::tempdir().unwrap();
        let spec = spec_by_id("qwen2.5-3b-instruct").unwrap();
        let p = dir.path().join("x.gguf");
        std::fs::write(&p, b"tiny").unwrap();
        assert!(verify(&p, spec).is_err());
        assert!(!p.exists(), "bad file removed");
    }
}
