use crate::{capture, hotkeys, state::AppState, Mode};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let capture_i = MenuItem::with_id(app, "capture", "Capture region", true, None::<&str>)?;
    let text_i = MenuItem::with_id(app, "text", "Grab text", true, None::<&str>)?;
    let pin_i = CheckMenuItem::with_id(app, "pinning", "Pin shots to shelf", true, crate::shelf::pinning(app), None::<&str>)?;
    let history_i = MenuItem::with_id(app, "history", "History…", true, None::<&str>)?;
    let settings_i = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &capture_i,
            &text_i,
            &pin_i,
            &PredefinedMenuItem::separator(app)?,
            &history_i,
            &settings_i,
            &quit_i,
        ],
    )?;
    *app.state::<AppState>().pin_item.lock().unwrap() = Some(pin_i);
    *app.state::<AppState>().tray_items.lock().unwrap() = Some((capture_i, text_i));
    refresh_labels(app);

    // The "hidden icon" (src-tauri/icons/tray.png, 64x64): the stars alone on transparency,
    // cropped tight for crisp HiDPI scaling across Windows/macOS/Linux tray slots. The app icon
    // keeps its sky background for taskbar/Start. Embedded at compile time; build.rs watches icons/.
    TrayIconBuilder::with_id("main")
        .icon(tauri::include_image!("icons/tray.png"))
        .tooltip("Sky Shot")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "capture" => capture::start(app, Mode::Capture),
            "text" => capture::start(app, Mode::Text),
            "pinning" => crate::shelf::set_pinning(app, !crate::shelf::pinning(app)),
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
