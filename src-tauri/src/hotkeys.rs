use crate::{capture, state::AppState, tray, Mode};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

pub fn parse(accel: &str) -> Result<Shortcut, String> {
    accel
        .parse::<Shortcut>()
        .map_err(|e| format!("Invalid shortcut \"{accel}\": {e}"))
}

/// A hotkey needs a modifier, except PrtSc and F-keys which are safe on their own.
pub fn validate(accel: &str) -> Result<Shortcut, String> {
    let sc = parse(accel)?;
    let standalone_ok = matches!(
        sc.key,
        Code::PrintScreen
            | Code::F1 | Code::F2 | Code::F3 | Code::F4 | Code::F5 | Code::F6
            | Code::F7 | Code::F8 | Code::F9 | Code::F10 | Code::F11 | Code::F12
    );
    if sc.mods.is_empty() && !standalone_ok {
        return Err("Add a modifier key (Ctrl, Alt, Shift or Win).".into());
    }
    Ok(sc)
}

/// Called by the global-shortcut plugin for every registered hotkey.
pub fn on_shortcut(app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    if event.state != ShortcutState::Pressed {
        return;
    }
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    if parse(&settings.hotkey_capture).ok().as_ref() == Some(shortcut) {
        capture::start(app, Mode::Capture);
    } else if parse(&settings.hotkey_text).ok().as_ref() == Some(shortcut) {
        capture::start(app, Mode::Text);
    }
}

/// (Re)register both hotkeys from settings. Failures (key taken by another app) are
/// left unregistered; the settings window shows them as inactive.
pub fn register_all(app: &AppHandle) {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let state = app.state::<AppState>();
    if state.hotkeys_paused.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let settings = state.settings.lock().unwrap().clone();
    for accel in [&settings.hotkey_capture, &settings.hotkey_text] {
        if let Ok(sc) = parse(accel)
            && let Err(e) = gs.register(sc) {
                eprintln!("[hotkeys] could not register {accel}: {e}");
            }
    }
    tray::refresh_labels(app);
}

pub fn is_active(app: &AppHandle, accel: &str) -> bool {
    parse(accel)
        .map(|sc| app.global_shortcut().is_registered(sc))
        .unwrap_or(false)
}

/// Change one hotkey. Test-registers it first so we can tell the user if it's taken.
pub fn set(app: &AppHandle, action: &str, accel: &str) -> Result<(), String> {
    let new = validate(accel)?;
    let state = app.state::<AppState>();
    let mut settings = state.settings.lock().unwrap().clone();
    let other = match action {
        "capture" => &settings.hotkey_text,
        "text" => &settings.hotkey_capture,
        _ => return Err(format!("Unknown action {action}")),
    };
    if parse(other).ok() == Some(new) {
        return Err("That shortcut is already used for the other action.".into());
    }

    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if gs.register(new).is_err() {
        register_all(app);
        return Err("That shortcut is already used by another app. Pick another one.".into());
    }
    let _ = gs.unregister(new);

    match action {
        "capture" => settings.hotkey_capture = accel.to_string(),
        _ => settings.hotkey_text = accel.to_string(),
    }
    crate::settings::save(app, &settings)?;
    *state.settings.lock().unwrap() = settings;
    register_all(app);
    Ok(())
}

/// Human-friendly label, e.g. "Ctrl+PrintScreen" -> "Ctrl+PrtSc", "Alt+KeyS" -> "Alt+S".
pub fn pretty(accel: &str) -> String {
    accel
        .split('+')
        .map(|part| match part {
            "PrintScreen" => "PrtSc".to_string(),
            "Super" | "Meta" => "Win".to_string(),
            p if p.starts_with("Key") && p.len() == 4 => p[3..].to_string(),
            p if p.starts_with("Digit") && p.len() == 6 => p[5..].to_string(),
            p => p.to_string(),
        })
        .collect::<Vec<_>>()
        .join("+")
}
