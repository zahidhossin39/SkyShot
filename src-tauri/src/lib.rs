mod ai;
mod applog;
mod capture;
mod history;
mod hotkeys;
mod ocr;
mod output;
mod screens;
mod scroll;
mod settings;
mod shelf;
mod state;
mod tray;
mod update;

use serde::{Deserialize, Serialize};
use state::AppState;
use std::sync::{atomic::Ordering, Mutex};
use tauri::{
    ipc::{InvokeBody, Request},
    AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
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
        .title("Shutter — Settings")
        .inner_size(820.0, 600.0)
        .resizable(false)
        .maximizable(false)
        .center()
        .build();
}

pub fn open_history(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("history") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, "history", WebviewUrl::App("history.html".into()))
        .title("Shutter — History")
        .inner_size(960.0, 680.0)
        .min_inner_size(520.0, 400.0)
        .center()
        .build();
}

/// Add a finished capture to history in the background (it runs OCR), and to the shelf if pinning is on.
fn remember(app: &AppHandle, rgba: Vec<u8>, width: u32, height: u32) {
    let app = app.clone();
    std::thread::spawn(move || {
        if shelf::pinning(&app) {
            shelf::add(&app, rgba.clone(), width, height);
        }
        if let Err(e) = history::add(&app, rgba, width, height) {
            crate::log!("[history] save failed: {e}");
            return;
        }
        let _ = app.emit_to("history", "history:changed", ());
    });
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
fn overlay_hide(app: AppHandle) {
    capture::hide(&app);
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

/// Re-read the selection with the user's AI provider. Only this one image is sent.
#[tauri::command]
async fn ai_reread(app: AppHandle, request: Request<'_>) -> Result<String, String> {
    let (rgba, w, h) = raw_image(&request)?;
    let provider = app.state::<AppState>().settings.lock().unwrap().ai_provider.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let key = ai::key(&provider)
            .ok_or_else(|| format!("No {} API key yet. Add one in Settings → AI re-read.", ai::label(&provider)))?;
        ai::reread(&provider, &key, &rgba, w, h)
    })
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
    crate::log!("[finish] {action} {w}x{h}, {} ms after hotkey", capture::since_hotkey(&app));
    capture::close(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let result = output::finish(&app, &action, &rgba, w, h);
        match &result {
            Err(e) => {
                crate::log!("[finish] {action} failed: {e}");
                output::notify(&app, "Something went wrong", e)
            }
            Ok(None) if action == "saveas" => {} // Save-as cancelled: nothing was finished
            Ok(_) => remember(&app, rgba, w, h),
        }
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Scrolling capture of the selection (`x, y, w, h` in this overlay's monitor pixels).
/// The overlay closes so the wheel reaches the real window under it.
#[tauri::command]
fn scroll_capture(app: AppHandle, window: WebviewWindow, x: u32, y: u32, w: u32, h: u32) -> Result<(), String> {
    let idx: usize = window.label().trim_start_matches("overlay-").parse().map_err(|_| "not an overlay")?;
    capture::close(&app);
    scroll::ACTIVE.store(true, std::sync::atomic::Ordering::SeqCst);
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(250)); // let the overlay disappear
        let back = app.cursor_position().ok();
        let parked = std::cell::Cell::new(None); // (x, y, points per pixel) where the cursor was put
        let result = (|| -> Result<image::RgbaImage, String> {
            let (mon, capture) = screens::grabber(&app, idx)?;
            let (mx, my) = (mon.x, mon.y);
            let grab = || capture().map(|img| image::imageops::crop_imm(&img, x, y, w, h).to_image());
            // Park the cursor over the area once; if the user moves it (or presses Esc), stop.
            // Cursor coordinates are points on macOS, pixels elsewhere.
            let k = mon.scale;
            let (cx, cy) = (mx + ((x + w / 2) as f64 / k) as i32, my + ((y + h / 2) as f64 / k) as i32);
            parked.set(Some((cx, cy, k)));
            scroll::move_cursor(cx, cy);
            // No "hands off" toast here: it slid into the area being captured and got stitched in.
            let go_on = |n| {
                if scroll::user_interrupted(cx, cy) {
                    return false;
                }
                scroll::wheel(n);
                std::thread::sleep(std::time::Duration::from_millis(60)); // let the scroll start
                !scroll::user_interrupted(cx, cy)
            };
            scroll::run_with(grab, go_on, scroll::SETTLE_MS)
        })();
        // Put the cursor back, unless the user already took it.
        if let (Some(p), Some((cx, cy, k))) = (back, parked.get())
            && !scroll::user_interrupted(cx, cy) {
                scroll::move_cursor((p.x / k) as i32, (p.y / k) as i32);
            }
        scroll::ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
        crate::log!("[scroll] {x},{y} {w}x{h} -> {:?}", result.as_ref().map(|i| i.dimensions()));
        match result {
            Ok(img) => {
                let (iw, ih) = img.dimensions();
                match output::copy_image(img.as_raw(), iw, ih) {
                    Ok(()) => output::notify(&app, "Long screenshot copied", &format!("{iw} × {ih}. Also saved to History.")),
                    Err(e) => output::notify(&app, "Something went wrong", &e),
                }
                remember(&app, img.into_raw(), iw, ih);
            }
            Err(e) => output::notify(&app, "Scrolling capture failed", &e),
        }
    });
    Ok(())
}

// ---------- pinning + shelf commands ----------

#[tauri::command]
fn get_pinning(app: AppHandle) -> bool {
    shelf::pinning(&app)
}

#[tauri::command]
fn set_pinning(app: AppHandle, enabled: bool) {
    shelf::set_pinning(&app, enabled);
}

#[tauri::command]
fn shelf_list(app: AppHandle) -> Vec<shelf::Item> {
    shelf::list(&app)
}

/// `dropped`: the shot was dragged into another app (its file is deleted a little later).
#[tauri::command]
fn shelf_remove(app: AppHandle, id: String, dropped: bool) {
    shelf::remove(&app, &id, dropped);
}

#[tauri::command]
fn shelf_clear(app: AppHandle) {
    shelf::clear(&app);
}

/// The shelf page reports its content height (logical px) so the window can fit it.
#[tauri::command]
fn shelf_fit(app: AppHandle, height: f64) {
    shelf::fit(&app, height);
}

/// Absolute path of a shelf PNG, for the OS drag.
#[tauri::command]
fn shelf_path(app: AppHandle, name: String) -> Result<String, String> {
    shelf::file_for(&app, &name).map(|p| p.display().to_string())
}

// ---------- history commands ----------

#[derive(Serialize)]
struct HistoryView {
    /// Folder holding the PNGs, for the page's asset-protocol URLs.
    dir: String,
    entries: Vec<history::Entry>,
}

#[tauri::command]
fn history_list(app: AppHandle) -> HistoryView {
    HistoryView { dir: history::dir(&app).display().to_string(), entries: history::list(&app) }
}

#[tauri::command]
fn history_star(app: AppHandle, id: String, starred: bool) -> Result<(), String> {
    history::set_star(&app, &id, starred)
}

#[tauri::command]
fn history_delete(app: AppHandle, id: String) -> Result<(), String> {
    history::delete(&app, &id)
}

#[tauri::command]
async fn history_copy(app: AppHandle, id: String) -> Result<(), String> {
    let img = image::open(history::image_path(&app, &id)?).map_err(|e| e.to_string())?.to_rgba8();
    output::copy_image(img.as_raw(), img.width(), img.height())
}

#[tauri::command]
fn history_reveal(app: AppHandle, id: String) -> Result<(), String> {
    let path = history::image_path(&app, &id)?;
    // `/select,<path>` must be one argument for explorer to highlight the file.
    std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Async on purpose: building a window inside a sync command deadlocks WebView2 on Windows,
/// which left the History window unable to close.
#[tauri::command]
async fn open_history_window(app: AppHandle) {
    open_history(&app);
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
    history_days: u32,
    ai_provider: String,
    /// "…ab12" for the saved key, or empty. The key itself never goes to the page.
    ai_key_hint: String,
}

fn settings_view(app: &AppHandle) -> SettingsView {
    let s = app.state::<AppState>().settings.lock().unwrap().clone();
    SettingsView {
        capture_active: hotkeys::is_active(app, &s.hotkey_capture),
        text_active: hotkeys::is_active(app, &s.hotkey_text),
        save_dir: settings::save_dir(app, &s).display().to_string(),
        save_dir_is_default: s.save_dir.is_empty(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        history_days: s.history_days,
        ai_key_hint: ai::key(&s.ai_provider).map(|k| ai::hint(&k)).unwrap_or_default(),
        ai_provider: s.ai_provider,
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

/// Settings → Settings page → Show logs: the folder with shutter.log, for bug reports.
#[tauri::command]
fn open_logs() -> Result<(), String> {
    let dir = applog::dir().ok_or("No log folder")?;
    std::process::Command::new("explorer")
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
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

/// Also applies to shots already in history (shortening deletes older ones, except starred).
#[tauri::command]
fn set_history_days(app: AppHandle, days: u32) -> Result<SettingsView, String> {
    if days > 3650 {
        return Err("Pick up to 10 years".into());
    }
    let state = app.state::<AppState>();
    let mut s = state.settings.lock().unwrap().clone();
    s.history_days = days;
    settings::save(&app, &s)?;
    *state.settings.lock().unwrap() = s;
    history::prune(&app);
    let _ = app.emit_to("history", "history:changed", ());
    Ok(settings_view(&app))
}

/// Pick the provider and save (or, with an empty key, keep / remove) its key.
#[tauri::command]
fn set_ai(app: AppHandle, provider: String, key: Option<String>) -> Result<SettingsView, String> {
    if !ai::PROVIDERS.contains(&provider.as_str()) {
        return Err("Unknown AI provider".into());
    }
    match key.as_deref().map(str::trim) {
        Some("") => ai::remove_key(&provider),
        Some(k) => ai::save_key(&provider, k)?,
        None => {} // only switching provider
    }
    let state = app.state::<AppState>();
    let mut s = state.settings.lock().unwrap().clone();
    s.ai_provider = provider;
    settings::save(&app, &s)?;
    *state.settings.lock().unwrap() = s;
    Ok(settings_view(&app))
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
        // History images: shot://<id>.png / <id>.thumb.png (http://shot.localhost/… on Windows).
        // Read with plain file access, not the asset protocol: that one compares canonical paths,
        // and Windows can keep files somewhere else (a launch from a packaged app virtualizes
        // AppData), which gave every thumbnail a 403. Off the main thread: History loads dozens.
        .register_asynchronous_uri_scheme_protocol("shot", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            std::thread::spawn(move || {
                let name = request.uri().path().trim_start_matches('/').to_string();
                let file = history::file_for(&app, &name).and_then(|p| std::fs::read(p).map_err(|e| e.to_string()));
                let response = match file {
                    Ok(bytes) => tauri::http::Response::builder()
                        .header("Content-Type", "image/png")
                        .header("Cache-Control", "max-age=31536000, immutable") // an id's image never changes
                        .body(bytes),
                    Err(_) => tauri::http::Response::builder().status(404).body(Vec::new()),
                };
                responder.respond(response.unwrap());
            });
        })
        // Overlay frame where WebView2 shared memory doesn't exist (macOS, Linux): the page
        // fetches frame://… and gets the raw RGBA, no JSON. Keyed by the asking webview, so a
        // window can only ever take its own frame.
        .register_uri_scheme_protocol("frame", |ctx, _request| {
            let label = ctx.webview_label().to_string();
            match capture::take_frame(ctx.app_handle(), &label) {
                Some(f) => tauri::http::Response::builder()
                    .header("Content-Type", "application/octet-stream")
                    .header("Cache-Control", "no-store")
                    .body(f.rgba),
                None => tauri::http::Response::builder().status(404).body(Vec::new()),
            }
            .unwrap()
        })
        // Shelf thumbnails: shelf://<id>.thumb.png (see shelf.rs).
        .register_asynchronous_uri_scheme_protocol("shelf", shelf::protocol)
        .plugin(tauri_plugin_drag::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .setup(|app| {
            if let Ok(dir) = app.path().app_data_dir() {
                applog::init(dir.join("logs"));
            }
            // A tray utility: no Dock icon, and its windows may float over full-screen apps.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            #[cfg(target_os = "linux")]
            if let Ok(dir) = app.path().resource_dir() {
                ocr::set_model_dir(dir.join("ocr-models"));
            }
            let handle = app.handle();
            let first_run = !settings::exists(handle);
            let s = settings::load(handle);
            if first_run {
                let _ = settings::save(handle, &s);
            }
            app.manage(AppState {
                settings: Mutex::new(s),
                frames: Mutex::new(Default::default()),
                shelf: Mutex::new(Vec::new()),
                pin_item: Mutex::new(None),
                session: Mutex::new(None),
                hotkeys_paused: Default::default(),
                tray_items: Mutex::new(None),
            });
            shelf::cleanup_on_start(handle);
            tray::create(handle)?;
            hotkeys::register_all(handle);
            capture::warm_up(handle);
            update::check_on_start(handle);
            // Retention also runs on start, so shots expire even without new captures.
            let h = handle.clone();
            std::thread::spawn(move || history::prune(&h));
            // A tray app is invisible; show settings once so the user knows it's running.
            if first_run {
                open_settings(handle);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            update::check_update,
            update::install_update,
            overlay_pending,
            overlay_frame,
            overlay_shown,
            overlay_hide,
            overlay_close,
            ocr_image,
            copy_text,
            ai_reread,
            set_ai,
            open_url,
            grab_text,
            finish,
            get_pinning,
            set_pinning,
            shelf_list,
            shelf_remove,
            shelf_clear,
            shelf_fit,
            shelf_path,
            scroll_capture,
            history_list,
            history_star,
            history_delete,
            history_copy,
            history_reveal,
            open_history_window,
            set_history_days,
            get_settings,
            pause_hotkeys,
            set_hotkey,
            reset_hotkeys,
            pick_save_dir,
            reset_save_dir,
            open_save_dir,
            open_logs,
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
