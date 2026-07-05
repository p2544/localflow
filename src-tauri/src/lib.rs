//! LocalFlow Tauri shell: tray, global hotkey, pill overlay, IPC commands.

mod commands;
mod hotkey;
mod tray;

use localflow_core::{PipelineEvent, PipelineHandle, Settings};
use parking_lot::Mutex;
use tauri::{Emitter, Manager};

pub struct AppState {
    pub pipeline: PipelineHandle,
    pub settings: Mutex<Settings>,
    /// Latched flag for hands-free toggle mode.
    pub toggle_active: Mutex<bool>,
    pub download_cancel: Mutex<std::collections::HashSet<String>>,
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let settings = Settings::load();
    let pipeline = PipelineHandle::spawn(settings.clone());
    let events = pipeline.events.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(AppState {
            pipeline,
            settings: Mutex::new(settings),
            toggle_active: Mutex::new(false),
            download_cancel: Mutex::new(Default::default()),
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_settings,
            commands::list_mics,
            commands::model_catalog,
            commands::model_status,
            commands::download_model,
            commands::cancel_download,
            commands::delete_model,
            commands::history_list,
            commands::history_delete,
            commands::history_clear,
            commands::dictionary_list,
            commands::dictionary_add,
            commands::dictionary_remove,
            commands::start_dictation,
            commands::stop_dictation,
            commands::clean_text_preview,
            commands::open_settings_pane,
        ])
        .setup(|app| {
            tray::setup_tray(app.handle())?;
            hotkey::register_from_settings(app.handle())?;

            // Forward pipeline events to the webviews and drive the pill window.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                while let Ok(ev) = events.recv() {
                    match &ev {
                        PipelineEvent::RecordingStarted => show_pill(&handle),
                        PipelineEvent::Done { .. }
                        | PipelineEvent::Empty
                        | PipelineEvent::Error { .. } => hide_pill_soon(&handle, &ev),
                        _ => {}
                    }
                    let _ = handle.emit("pipeline-event", &ev);
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window hides it; the app lives in the tray.
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building LocalFlow")
        .run(|app, event| {
            // macOS: clicking the Dock icon with no visible windows should
            // bring the main window back (the app is tray-resident).
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            let _ = (app, &event);
        });
}

fn show_pill(app: &tauri::AppHandle) {
    if let Some(pill) = app.get_webview_window("pill") {
        position_pill(&pill);
        let _ = pill.show();
    }
}

/// Keep terminal states visible briefly so the user sees the result flash.
fn hide_pill_soon(app: &tauri::AppHandle, ev: &PipelineEvent) {
    let delay = match ev {
        PipelineEvent::Error { .. } => 2500,
        _ => 900,
    };
    if let Some(pill) = app.get_webview_window("pill") {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(delay));
            let _ = pill.hide();
        });
    }
}

/// Bottom-center of the monitor with the cursor.
fn position_pill(pill: &tauri::WebviewWindow) {
    let Ok(Some(monitor)) = pill
        .current_monitor()
        .or_else(|_| pill.primary_monitor())
    else {
        return;
    };
    let msize = monitor.size();
    let mpos = monitor.position();
    let wsize = pill.outer_size().unwrap_or(tauri::PhysicalSize {
        width: 260,
        height: 64,
    });
    let x = mpos.x + ((msize.width as i32 - wsize.width as i32) / 2);
    let y = mpos.y + msize.height as i32 - wsize.height as i32 - 48;
    let _ = pill.set_position(tauri::PhysicalPosition { x, y });
}
