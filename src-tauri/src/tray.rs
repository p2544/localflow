//! System tray / menu-bar icon and menu.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;

pub fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open LocalFlow", true, None::<&str>)?;
    let history = MenuItem::with_id(app, "history", "History", true, None::<&str>)?;
    let scratchpad = MenuItem::with_id(app, "scratchpad", "Scratchpad", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit LocalFlow", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &history, &scratchpad, &sep, &quit])?;

    // The tray icon declared in tauri.conf.json is built automatically only
    // when no code builds one; build explicitly so we control the menu.
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().ok_or(tauri::Error::UnknownPath)?)
        .tooltip("LocalFlow — hold your hotkey and speak")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            let show_main_at = |route: &str| {
                if let Some(w) = app.get_webview_window("main") {
                    use tauri::Emitter;
                    let _ = app.emit("navigate", route);
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            };
            match event.id().as_ref() {
                "open" => show_main_at("settings"),
                "history" => show_main_at("history"),
                "scratchpad" => show_main_at("scratchpad"),
                "quit" => {
                    let st = app.state::<crate::AppState>();
                    st.pipeline.shutdown();
                    app.exit(0);
                }
                _ => {}
            }
        })
        .build(app)?;
    Ok(())
}
