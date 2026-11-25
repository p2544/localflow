//! Model catalog + download manager (Hugging Face, sha256, resume).
//! The ONLY code in the app allowed to touch the network, and only when the
//! user explicitly requests a download.

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct ExtraFile {
    pub file_name: &'static str,
    pub url: &'static str,
    /// Hex sha256; empty = skip verification (size check only).
    pub sha256: &'static str,
    pub size_bytes: u64,
}

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
    /// Extra files required alongside `file_name`, in the same models dir —
    /// for a model shipped as several parts (e.g. a separate encoder/decoder
    /// pair plus a tokenizer file) rather than one file.
    pub extra_files: &'static [ExtraFile],
}

/// Curated catalog. Sizes are approximate for progress UI; hash empty where
/// upstream repos don't publish stable hashes (verified by size instead).
pub const CATALOG: &[ModelSpec] = &[
    ModelSpec {
        id: "pathumma-th-large-v3",
        kind: "asr",
        file_name: "ggml-pathumma-th-large-v3-q5_0.bin",
        url: "https://github.com/p2544/localflow/releases/download/models-thai-v1/ggml-pathumma-th-large-v3-q5_0.bin",
        sha256: "adbc51a20eb4fffc1d691613379b44e8194264468dced7efc1ccc80e119ee49b",
        size_bytes: 1_081_140_203,
        label: "Pathumma Thai Large-v3 Q5 (~1.1 GB)",
        note: "ดีที่สุดสำหรับภาษาไทย — NECTEC fine-tune (Apache-2.0). Thai-specialized.",
        extra_files: &[],
    },
    ModelSpec {
        id: "whisper-large-v3-turbo",
        kind: "asr",
        file_name: "ggml-large-v3-turbo-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        size_bytes: 574_041_195,
        label: "Whisper Large-v3 Turbo Q5 (fast, 547 MB)",
        note: "Recommended default — fast, strong across 100 languages.",
        extra_files: &[],
    },
    ModelSpec {
        id: "whisper-large-v3",
        kind: "asr",
        file_name: "ggml-large-v3-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-q5_0.bin",
        sha256: "d75795ecff3f83b5faa89d1900604ad8c780abd5739fae406de19f23ecd98ad1",
        size_bytes: 1_081_140_203,
        label: "Whisper Large-v3 Q5 (~1.1 GB)",
        note: "Highest-accuracy vanilla Whisper; slower than Turbo.",
        extra_files: &[],
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
        extra_files: &[],
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
        extra_files: &[],
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
            let mut bytes = file_len(models_dir, m.file_name);
            let mut installed = bytes > 0;
            for extra in m.extra_files {
                let b = file_len(models_dir, extra.file_name);
                installed = installed && b > 0;
                bytes += b;
            }
            ModelStatus {
                id: m.id.to_string(),
                // "installed" = every part present (partial files use .part).
                installed,
                bytes_on_disk: bytes,
            }
        })
        .collect()
}

fn file_len(models_dir: &Path, file_name: &str) -> u64 {
    std::fs::metadata(models_dir.join(file_name))
        .map(|md| md.len())
        .unwrap_or(0)
}

/// Blocking download with resume + progress callback (downloaded, total,
/// summed across `spec.file_name` and every `extra_files` entry). Writes to
/// `<file>.part`, verifies, then renames — a crash never leaves a half-file
/// that looks installed.
pub fn download(
    spec: &ModelSpec,
    models_dir: &Path,
    mut progress: impl FnMut(u64, u64),
    cancelled: &dyn Fn() -> bool,
) -> Result<PathBuf> {
    let grand_total = spec.size_bytes + spec.extra_files.iter().map(|f| f.size_bytes).sum::<u64>();
    let mut offset = 0u64;

    let primary = download_one(
        spec.url,
        spec.file_name,
        spec.sha256,
        spec.size_bytes,
        models_dir,
        offset,
        grand_total,
        &mut progress,
        cancelled,
    )?;
    offset += spec.size_bytes;

    for extra in spec.extra_files {
        download_one(
            extra.url,
            extra.file_name,
            extra.sha256,
            extra.size_bytes,
            models_dir,
            offset,
            grand_total,
            &mut progress,
            cancelled,
        )?;
        offset += extra.size_bytes;
    }

    Ok(primary)
}

#[allow(clippy::too_many_arguments)]
fn download_one(
    url: &str,
    file_name: &str,
    sha256: &str,
    size_bytes: u64,
    models_dir: &Path,
    offset: u64,
    grand_total: u64,
    progress: &mut dyn FnMut(u64, u64),
    cancelled: &dyn Fn() -> bool,
) -> Result<PathBuf> {
    let final_path = models_dir.join(file_name);
    if final_path.exists() {
        progress(offset + size_bytes, grand_total);
        return Ok(final_path);
    }
    let part_path = models_dir.join(format!("{file_name}.part"));
    let existing = std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let mut req = client.get(url);
    if existing > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={existing}-"));
    }
    let resp = req.send().context("download request failed")?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("download failed: HTTP {status}"));
    }
    let resuming = status == reqwest::StatusCode::PARTIAL_CONTENT;

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
        progress(offset + written, grand_total);
    }
    file.flush()?;
    drop(file);

    verify(&part_path, sha256, size_bytes)?;
    std::fs::rename(&part_path, &final_path)?;
    Ok(final_path)
}

fn verify(path: &Path, sha256: &str, size_bytes: u64) -> Result<()> {
    let md = std::fs::metadata(path)?;
    if !sha256.is_empty() {
        let mut hasher = Sha256::new();
        let mut f = std::fs::File::open(path)?;
        std::io::copy(&mut f, &mut hasher)?;
        let got = hex::encode(hasher.finalize());
        if got != sha256 {
            std::fs::remove_file(path).ok();
            return Err(anyhow!("sha256 mismatch: expected {sha256}, got {got}"));
        }
    } else if md.len() < size_bytes / 2 {
        std::fs::remove_file(path).ok();
        return Err(anyhow!("downloaded file suspiciously small ({} bytes)", md.len()));
    }
    Ok(())
}

pub fn delete_model(spec: &ModelSpec, models_dir: &Path) -> Result<()> {
    let mut names = vec![spec.file_name.to_string(), format!("{}.part", spec.file_name)];
    for extra in spec.extra_files {
        names.push(extra.file_name.to_string());
        names.push(format!("{}.part", extra.file_name));
    }
    for name in names {
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
        assert!(spec_by_id("whisper-large-v3-turbo").is_some());
        assert!(spec_by_id("pathumma-th-large-v3").is_some());
        assert!(spec_by_file("ggml-large-v3-turbo-q5_0.bin").is_some());
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
        assert!(verify(&p, spec.sha256, spec.size_bytes).is_err());
        assert!(!p.exists(), "bad file removed");
    }
}
