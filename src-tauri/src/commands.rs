//! Tauri IPC commands — thin wrappers over localflow-core.

use crate::AppState;
use localflow_core::history::{open_db, History, HistoryEntry};
use localflow_core::dictionary::Dictionary;
use localflow_core::models;
use localflow_core::settings::Settings;
use serde::Serialize;
use tauri::{Emitter, Manager, State};

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().clone()
}

#[tauri::command]
pub fn set_settings(
    app: tauri::AppHandle,
    state: State<AppState>,
    settings: Settings,
) -> CmdResult<()> {
    let hotkey_changed = {
        let mut cur = state.settings.lock();
        let changed = cur.hotkey != settings.hotkey;
        *cur = settings.clone();
        changed
    };
    settings.save().map_err(err)?;
    state.pipeline.reload_settings(settings.clone());
    if hotkey_changed {
        crate::hotkey::register(&app, &settings.hotkey).map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
pub fn list_mics() -> Vec<String> {
    localflow_core::audio::list_input_devices()
}

#[tauri::command]
pub fn model_catalog() -> Vec<models::ModelSpec> {
    models::CATALOG.to_vec()
}

#[tauri::command]
pub fn model_status() -> CmdResult<Vec<models::ModelStatus>> {
    let dir = Settings::models_dir().map_err(err)?;
    Ok(models::status(&dir))
}

#[derive(Clone, Serialize)]
struct DownloadProgress {
    id: String,
    downloaded: u64,
    total: u64,
}

/// Long-running: streams `model-download-progress` events, resolves when done.
#[tauri::command(async)]
pub fn download_model(app: tauri::AppHandle, id: String) -> CmdResult<()> {
    let spec = models::spec_by_id(&id).ok_or("unknown model id")?;
    let dir = Settings::models_dir().map_err(err)?;
    {
        let st = app.state::<AppState>();
        st.download_cancel.lock().remove(&id);
    }
    let app2 = app.clone();
    let id2 = id.clone();
    let mut last_emit = std::time::Instant::now();
    let cancelled = {
        let app = app.clone();
        let id = id.clone();
        move || {
            let st = app.state::<AppState>();
            let c = st.download_cancel.lock().contains(&id);
            c
        }
    };
    models::download(
        spec,
        &dir,
        move |downloaded, total| {
            if last_emit.elapsed().as_millis() > 150 {
                last_emit = std::time::Instant::now();
                let _ = app2.emit(
                    "model-download-progress",
                    DownloadProgress { id: id2.clone(), downloaded, total },
                );
            }
        },
        &cancelled,
    )
    .map_err(err)?;
    let _ = app.emit(
        "model-download-progress",
        DownloadProgress { id, downloaded: spec.size_bytes, total: spec.size_bytes },
    );
    Ok(())
}

#[tauri::command]
pub fn cancel_download(state: State<AppState>, id: String) {
    state.download_cancel.lock().insert(id);
}

#[tauri::command]
pub fn delete_model(id: String) -> CmdResult<()> {
    let spec = models::spec_by_id(&id).ok_or("unknown model id")?;
    let dir = Settings::models_dir().map_err(err)?;
    models::delete_model(spec, &dir).map_err(err)
}

#[tauri::command]
pub fn history_list(query: String, limit: usize) -> CmdResult<Vec<HistoryEntry>> {
    let conn = open_db().map_err(err)?;
    History::list(&conn, &query, limit.clamp(1, 500)).map_err(err)
}

#[tauri::command]
pub fn history_delete(id: i64) -> CmdResult<()> {
    let conn = open_db().map_err(err)?;
    History::delete(&conn, id).map_err(err)
}

#[tauri::command]
pub fn history_clear() -> CmdResult<()> {
    let conn = open_db().map_err(err)?;
    History::clear(&conn).map_err(err)
}

#[tauri::command]
pub fn dictionary_list() -> CmdResult<Vec<String>> {
    let conn = open_db().map_err(err)?;
    Dictionary::all(&conn).map_err(err)
}

#[tauri::command]
pub fn dictionary_add(word: String) -> CmdResult<Vec<String>> {
    let conn = open_db().map_err(err)?;
    Dictionary::add(&conn, &word).map_err(err)?;
    Dictionary::all(&conn).map_err(err)
}

#[tauri::command]
pub fn dictionary_remove(word: String) -> CmdResult<Vec<String>> {
    let conn = open_db().map_err(err)?;
    Dictionary::remove(&conn, &word).map_err(err)?;
    Dictionary::all(&conn).map_err(err)
}

/// UI button path (scratchpad / test dictation) — same pipeline as the
/// hotkey, but capture-only: the text arrives via the Done event instead of
/// being injected (the app's own window is focused, injecting would double).
#[tauri::command]
pub fn start_dictation(state: State<AppState>, capture_only: Option<bool>) {
    state
        .pipeline
        .set_capture_only(capture_only.unwrap_or(true));
    state.pipeline.start_recording();
}

#[tauri::command]
pub fn stop_dictation(state: State<AppState>, discard: bool) {
    *state.toggle_active.lock() = false;
    state.pipeline.stop_recording(discard);
}

/// Runs the cleanup layer on arbitrary text — used by the settings UI to
/// preview cleanup and by the M2 acceptance tests.
#[tauri::command(async)]
pub fn clean_text_preview(state: State<AppState>, raw: String) -> CmdResult<String> {
    state.pipeline.clean_text(raw).map_err(err)
}

/// macOS: shows the system Accessibility-trust prompt so LocalFlow appears
/// in the Privacy & Security list; returns whether trust is granted.
/// Always true on other platforms.
#[tauri::command]
pub fn request_accessibility() -> bool {
    localflow_core::inject::request_accessibility_trust()
}

/// Deep-links the OS settings pane the user must grant (macOS only today).
#[tauri::command]
pub fn open_settings_pane(pane: String) -> CmdResult<()> {
    #[cfg(target_os = "macos")]
    {
        let url = match pane.as_str() {
            "microphone" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
            }
            "accessibility" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
            "input-monitoring" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
            }
            _ => return Err("unknown pane".into()),
        };
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(err)?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pane;
    }
    Ok(())
}
