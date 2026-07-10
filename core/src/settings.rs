use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How final text reaches the focused app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    /// Clipboard set + synthetic paste keystroke, clipboard restored after.
    #[default]
    Paste,
    /// Per-character synthetic keystrokes (Windows KEYEVENTF_UNICODE etc.).
    Type,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyMode {
    /// Hold to record, release to transcribe.
    #[default]
    PushToTalk,
    /// Press once to start, press again to stop.
    Toggle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Accelerator in Tauri global-shortcut syntax.
    pub hotkey: String,
    pub hotkey_mode: HotkeyMode,
    /// BCP-47-ish whisper language code ("en", "th", ...) or "auto".
    pub language: String,
    /// Whisper model file name inside the models dir.
    pub asr_model: String,
    /// LLM GGUF file name inside the models dir; empty disables cleanup.
    pub llm_model: String,
    /// Master switch for the LLM cleanup pass (raw-transcript mode when false).
    pub cleanup_enabled: bool,
    pub output_mode: OutputMode,
    /// Preferred input device name; empty = system default.
    pub mic_device: String,
    pub launch_at_login: bool,
    /// Keep dictation history in SQLite.
    pub history_enabled: bool,
    /// Unload model contexts after this many seconds idle; 0 = keep warm forever.
    pub low_memory_unload_secs: u64,
    /// Threads for whisper/llama; 0 = auto (physical cores, capped).
    pub threads: usize,
    pub onboarding_done: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: "CommandOrControl+Shift+Space".into(),
            hotkey_mode: HotkeyMode::PushToTalk,
            language: "en".into(),
            asr_model: "ggml-large-v3-turbo-q5_0.bin".into(),
            llm_model: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
            cleanup_enabled: true,
            output_mode: OutputMode::Paste,
            mic_device: String::new(),
            launch_at_login: false,
            history_enabled: true,
            low_memory_unload_secs: 0,
            threads: 0,
            onboarding_done: false,
        }
    }
}

impl Settings {
    pub fn app_data_dir() -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("ai", "LocalFlow", "LocalFlow")
            .context("cannot determine app data dir")?;
        let dir = dirs.data_dir().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn models_dir() -> Result<PathBuf> {
        let dir = Self::app_data_dir()?.join("models");
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn settings_path() -> Result<PathBuf> {
        Ok(Self::app_data_dir()?.join("settings.json"))
    }

    pub fn load() -> Self {
        let Ok(path) = Self::settings_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::settings_path()?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn effective_threads(&self) -> usize {
        if self.threads > 0 {
            return self.threads;
        }
        std::thread::available_parallelism()
            .map(|n| n.get().min(8))
            .unwrap_or(4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip_json() {
        let s = Settings::default();
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.hotkey, s.hotkey);
        assert_eq!(back.output_mode, OutputMode::Paste);
    }

    #[test]
    fn unknown_fields_do_not_break_load() {
        let raw = r#"{"hotkey":"F9","future_field":123}"#;
        let s: Settings = serde_json::from_str(raw).unwrap();
        assert_eq!(s.hotkey, "F9");
        assert!(s.cleanup_enabled);
    }
}
