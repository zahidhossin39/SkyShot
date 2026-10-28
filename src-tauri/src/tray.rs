use crate::{capture, hotkeys, state::AppState, Mode};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let capture_i = MenuItem::with_id(app, "capture", "Capture region", true, None::<&str>)?;
    let text_i = MenuItem::with_id(app, "text", "Grab text", true, None::<&str>)?;
    let history_i = MenuItem::with_id(app, "history", "History…", true, None::<&str>)?;
    let settings_i = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &capture_i,
            &text_i,
            &PredefinedMenuItem::separator(app)?,
            &history_i,
            &settings_i,
            &quit_i,
        ],
    )?;
    *app.state::<AppState>().tray_items.lock().unwrap() = Some((capture_i, text_i));
    refresh_labels(app);

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("Screenshot App")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "capture" => capture::start(app, Mode::Capture),
            "text" => capture::start(app, Mode::Text),
            "history" => crate::open_history(app),
            "settings" => crate::open_settings(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                capture::start(tray.app_handle(), Mode::Capture);
            }
        })
        .build(app)?;
    Ok(())
}

/// Show the current hotkeys next to the menu items.
pub fn refresh_labels(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap().clone();
    if let Some((capture_i, text_i)) = state.tray_items.lock().unwrap().as_ref() {
        let _ = capture_i.set_text(format!("Capture region\t{}", hotkeys::pretty(&settings.hotkey_capture)));
        let _ = text_i.set_text(format!("Grab text\t{}", hotkeys::pretty(&settings.hotkey_text)));
    }
}
