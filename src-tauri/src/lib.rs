mod capture;
mod hotkeys;
mod ocr;
mod output;
mod settings;
mod state;
mod tray;

use serde::{Deserialize, Serialize};
use state::AppState;
use std::sync::{atomic::Ordering, Mutex};
use tauri::{
    ipc::{InvokeBody, Request},
    AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Normal capture: select, edit, copy/save.
    Capture,
    /// Instant text grab: select -> OCR -> clipboard.
    Text,
}

pub fn open_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("Screenshot App — Settings")
        .inner_size(520.0, 520.0)
        .resizable(false)
        .maximizable(false)
        .center()
        .build();
}

/// Raw RGBA image sent from the overlay, with its size in headers.
fn raw_image(request: &Request<'_>) -> Result<(Vec<u8>, u32, u32), String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected raw image bytes".into());
    };
    let header = |name: &str| -> Result<u32, String> {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| format!("missing header {name}"))
    };
    let (w, h) = (header("x-width")?, header("x-height")?);
    if bytes.len() != (w * h * 4) as usize {
        return Err("image size does not match".into());
    }
    Ok((bytes.clone(), w, h))
}

// ---------- overlay commands ----------

#[tauri::command]
fn overlay_pending(app: AppHandle, window: WebviewWindow) -> Option<capture::OpenPayload> {
    capture::pending(&app, window.label())
}

#[tauri::command]
async fn overlay_frame(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    let frame = capture::take_frame(&app, window.label()).ok_or("no frame")?;
    // Async command, so waiting here doesn't block the main thread `with_webview` runs on.
    let (tx, rx) = std::sync::mpsc::channel();
    window
        .with_webview(move |wv| {
            let _ = tx.send(capture::post_frame(&wv, &frame.rgba));
        })
        .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}

#[tauri::command]
fn overlay_shown(app: AppHandle, window: WebviewWindow) -> u128 {
    capture::shown(&app, &window)
}

#[tauri::command]
fn overlay_close(app: AppHandle) {
    capture::close(&app);
}

#[tauri::command]
async fn ocr_image(request: Request<'_>) -> Result<ocr::Ocr, String> {
    let (rgba, w, h) = raw_image(&request)?;
    tauri::async_runtime::spawn_blocking(move || ocr::recognize(&rgba, w, h))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn copy_text(text: String) -> Result<(), String> {
    output::copy_text(&text)
}

/// Smart action: open a link found by OCR in the default browser. The text came from a
/// screenshot and could be anything, so only http(s) links, passed as an argument (no shell).
#[tauri::command]
fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    if !is_web_link(&url) {
        return Err("Not a web link".into());
    }
    capture::close(&app); // the overlay is always on top and would hide the browser
    std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", &url])
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn is_web_link(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://"))
        && !url.chars().any(|c| c.is_whitespace() || c.is_control() || c == '"')
}

#[cfg(test)]
#[test]
fn only_web_links_open() {
    assert!(is_web_link("https://tauri.app/start"));
    assert!(is_web_link("HTTP://example.com/a?b=1"));
    for bad in ["file:///C:/Windows/System32/calc.exe", "javascript:alert(1)", "ms-settings:", "https://a.io\" & calc", "https://a b"] {
        assert!(!is_web_link(bad), "{bad}");
    }
}

/// Instant text grab: close overlay right away, then OCR and copy.
#[tauri::command]
async fn grab_text(app: AppHandle, request: Request<'_>) -> Result<(), String> {
    let (rgba, w, h) = raw_image(&request)?;
    capture::close(&app);
    tauri::async_runtime::spawn_blocking(move || {
        match ocr::recognize(&rgba, w, h).map(|o| o.text) {
            Ok(text) if !text.trim().is_empty() => {
                output::copy_text(&text)?;
                let preview: String = text.chars().take(80).collect();
                output::notify(&app, &format!("Copied {} characters", text.chars().count()), &preview);
            }
            Ok(_) => output::notify(&app, "No text found", "Try selecting a larger area."),
            Err(e) => output::notify(&app, "Text extraction failed", &e),
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Copy / Save / Both / Save as. Closes the overlay first so it feels instant
/// (and so the Save-as dialog isn't hidden behind it).
#[tauri::command]
async fn finish(app: AppHandle, request: Request<'_>) -> Result<Option<String>, String> {
    let (rgba, w, h) = raw_image(&request)?;
    let action = request
        .headers()
        .get("x-action")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("copy")
        .to_string();
    capture::close(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let result = output::finish(&app, &action, &rgba, w, h);
        if let Err(e) = &result {
            output::notify(&app, "Something went wrong", e);
        }
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------- settings commands ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsView {
    hotkey_capture: String,
    hotkey_text: String,
    capture_active: bool,
    text_active: bool,
    save_dir: String,
    save_dir_is_default: bool,
    autostart: bool,
}

fn settings_view(app: &AppHandle) -> SettingsView {
    let s = app.state::<AppState>().settings.lock().unwrap().clone();
    SettingsView {
        capture_active: hotkeys::is_active(app, &s.hotkey_capture),
        text_active: hotkeys::is_active(app, &s.hotkey_text),
        save_dir: settings::save_dir(app, &s).display().to_string(),
        save_dir_is_default: s.save_dir.is_empty(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        hotkey_capture: s.hotkey_capture,
        hotkey_text: s.hotkey_text,
    }
}

#[tauri::command]
fn get_settings(app: AppHandle) -> SettingsView {
    settings_view(&app)
}

#[tauri::command]
fn pause_hotkeys(app: AppHandle, paused: bool) {
    app.state::<AppState>().hotkeys_paused.store(paused, Ordering::SeqCst);
    hotkeys::register_all(&app);
}

#[tauri::command]
fn set_hotkey(app: AppHandle, action: String, accel: String) -> Result<SettingsView, String> {
    hotkeys::set(&app, &action, &accel)?;
    Ok(settings_view(&app))
}

#[tauri::command]
fn reset_hotkeys(app: AppHandle) -> Result<SettingsView, String> {
    let state = app.state::<AppState>();
    let mut s = state.settings.lock().unwrap().clone();
    let d = settings::Settings::default();
    s.hotkey_capture = d.hotkey_capture;
    s.hotkey_text = d.hotkey_text;
    settings::save(&app, &s)?;
    *state.settings.lock().unwrap() = s;
    hotkeys::register_all(&app);
    Ok(settings_view(&app))
}

#[tauri::command]
async fn pick_save_dir(app: AppHandle) -> Result<SettingsView, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = app.dialog().file().blocking_pick_folder();
    if let Some(dir) = picked.and_then(|p| p.into_path().ok()) {
        let state = app.state::<AppState>();
        let mut s = state.settings.lock().unwrap().clone();
        s.save_dir = dir.display().to_string();
        settings::save(&app, &s)?;
        *state.settings.lock().unwrap() = s;
    }
    Ok(settings_view(&app))
}

#[tauri::command]
fn reset_save_dir(app: AppHandle) -> Result<SettingsView, String> {
    let state = app.state::<AppState>();
    let mut s = state.settings.lock().unwrap().clone();
    s.save_dir.clear();
    settings::save(&app, &s)?;
    *state.settings.lock().unwrap() = s;
    Ok(settings_view(&app))
}

#[tauri::command]
fn open_save_dir(app: AppHandle) -> Result<(), String> {
    let s = app.state::<AppState>().settings.lock().unwrap().clone();
    let dir = settings::save_dir(&app, &s);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<SettingsView, String> {
    let al = app.autolaunch();
    if enabled { al.enable() } else { al.disable() }.map_err(|e| e.to_string())?;
    Ok(settings_view(&app))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be first: a second launch forwards its args here instead of starting again.
        // `--capture` / `--text` let other tools (or tests) trigger a capture; `--close` ends it.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if argv.iter().any(|a| a == "--capture") {
                capture::start(app, Mode::Capture);
            } else if argv.iter().any(|a| a == "--text") {
                capture::start(app, Mode::Text);
            } else if argv.iter().any(|a| a == "--close") {
                capture::close(app);
            } else {
                open_settings(app);
            }
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(hotkeys::on_shortcut)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .setup(|app| {
            let handle = app.handle();
            let first_run = !settings::exists(handle);
            let s = settings::load(handle);
            if first_run {
                let _ = settings::save(handle, &s);
            }
            app.manage(AppState {
                settings: Mutex::new(s),
                frames: Mutex::new(Default::default()),
                session: Mutex::new(None),
                hotkeys_paused: Default::default(),
                tray_items: Mutex::new(None),
            });
            tray::create(handle)?;
            hotkeys::register_all(handle);
            capture::warm_up(handle);
            // A tray app is invisible; show settings once so the user knows it's running.
            if first_run {
                open_settings(handle);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            overlay_pending,
            overlay_frame,
            overlay_shown,
            overlay_close,
            ocr_image,
            copy_text,
            open_url,
            grab_text,
            finish,
            get_settings,
            pause_hotkeys,
            set_hotkey,
            reset_hotkeys,
            pick_save_dir,
            reset_save_dir,
            open_save_dir,
            set_autostart,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // Keep running in the tray when the settings window closes.
            // `app.exit(0)` from the tray menu has a code, so it still quits.
            if let RunEvent::ExitRequested { api, code: None, .. } = event {
                api.prevent_exit();
            }
        });
}
