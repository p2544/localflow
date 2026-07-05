//! Global hotkey wiring: push-to-talk (Pressed/Released) and hands-free
//! toggle, driven by the settings-configured accelerator.

use crate::AppState;
use localflow_core::settings::HotkeyMode;
use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub fn register_from_settings(app: &tauri::AppHandle) -> anyhow::Result<()> {
    let (accel, _mode) = {
        let st = app.state::<AppState>();
        let s = st.settings.lock();
        (s.hotkey.clone(), s.hotkey_mode)
    };
    register(app, &accel)
}

/// Replaces any existing registration with `accel`. Mode is read at event
/// time so switching PTT/toggle needs no re-register.
pub fn register(app: &tauri::AppHandle, accel: &str) -> anyhow::Result<()> {
    let gs = app.global_shortcut();
    gs.unregister_all()?;

    let shortcut: Shortcut = accel
        .parse()
        .map_err(|e| anyhow::anyhow!("bad hotkey '{accel}': {e}"))?;

    gs.on_shortcut(shortcut, move |app, _sc, event| {
        let st = app.state::<AppState>();
        let mode = st.settings.lock().hotkey_mode;
        match (mode, event.state()) {
            (HotkeyMode::PushToTalk, ShortcutState::Pressed) => {
                st.pipeline.start_recording();
            }
            (HotkeyMode::PushToTalk, ShortcutState::Released) => {
                st.pipeline.stop_recording(false);
            }
            (HotkeyMode::Toggle, ShortcutState::Pressed) => {
                let mut active = st.toggle_active.lock();
                if *active {
                    st.pipeline.stop_recording(false);
                    *active = false;
                } else {
                    st.pipeline.start_recording();
                    *active = true;
                }
            }
            (HotkeyMode::Toggle, ShortcutState::Released) => {}
        }
    })?;
    Ok(())
}
