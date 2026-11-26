//! The shelf: when pinning is on, every finished shot also lands on a small always-on-top strip
//! at the screen edge. Thumbnails can be dragged out into any app (a native OS drag of a PNG,
//! started by the page through tauri-plugin-drag); a successful drop removes the shot.
//!
//! Pixels live on disk (`<cache>/pins/<id>.png` plus a small `<id>.thumb.png`), so memory stays
//! flat however many shots pile up. Nothing here is Windows-specific.

use crate::{settings, state::AppState};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

pub const LABEL: &str = "shelf";
/// Window width in logical px: a 144 px thumbnail plus padding and room for the shadow.
const WIN_W: f64 = 176.0;
/// Gap between the shelf and the screen edge, logical px.
const MARGIN: f64 = 8.0;
/// Thumbnails are stored at 2x their on-screen width so they stay sharp on high-DPI screens.
const THUMB_W: u32 = 288;
const THUMB_MAX_H: u32 = 400;
/// How long a dropped shot's file stays on disk: the target app may still be reading it.
const KEEP_AFTER_DROP: Duration = Duration::from_secs(180);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    #[default]
    Left,
    Right,
}

#[derive(Clone, Serialize)]
pub struct Item {
    pub id: String,
    pub width: u32,
    pub height: u32,
}

/// Which screen edge the shelf belongs to, given where its centre is on the monitor.
/// Dropped exactly in the middle counts as left (the default side).
pub fn snap_side(center_x: f64, mon_x: f64, mon_w: f64) -> Side {
    if center_x > mon_x + mon_w / 2.0 { Side::Right } else { Side::Left }
}

/// Top-left of a shelf window of `win` (w, h) docked to `side` and vertically centred
/// on the monitor `(x, y, w, h)`. All values in the same unit.
pub fn dock_pos(side: Side, mon: (f64, f64, f64, f64), win: (f64, f64), margin: f64) -> (f64, f64) {
    let x = match side {
        Side::Left => mon.0 + margin,
        Side::Right => mon.0 + mon.2 - win.0 - margin,
    };
    (x, mon.1 + (mon.3 - win.1) / 2.0)
}

fn dir(app: &AppHandle) -> PathBuf {
    app.path().app_cache_dir().unwrap_or_else(|_| std::env::temp_dir()).join("pins")
}

/// Leftovers from a previous run (the shelf starts empty).
pub fn cleanup_on_start(app: &AppHandle) {
    let _ = std::fs::remove_dir_all(dir(app));
}

fn next_id() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    format!("{ms}-{}", N.fetch_add(1, Ordering::Relaxed))
}

/// Thumbnail size: 288 px wide, never taller than 400 (long scrolling shots shrink instead).
pub fn thumb_size(w: u32, h: u32) -> (u32, u32) {
    let (w, h) = (w.max(1) as f64, h.max(1) as f64);
    let scale = (THUMB_W as f64 / w).min(THUMB_MAX_H as f64 / h).min(1.0);
    (((w * scale).round() as u32).max(1), ((h * scale).round() as u32).max(1))
}

fn write_files(app: &AppHandle, id: &str, rgba: Vec<u8>, w: u32, h: u32) -> Result<(), String> {
    let dir = dir(app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let img = image::RgbaImage::from_raw(w, h, rgba).ok_or("bad image size")?;
    img.save(dir.join(format!("{id}.png"))).map_err(|e| e.to_string())?;
    let (tw, th) = thumb_size(w, h);
    image::imageops::resize(&img, tw, th, image::imageops::FilterType::Triangle)
        .save(dir.join(format!("{id}.thumb.png")))
        .map_err(|e| e.to_string())
}

/// Add a finished shot to the shelf (call from a background thread: it encodes PNGs).
pub fn add(app: &AppHandle, rgba: Vec<u8>, w: u32, h: u32) {
    let id = next_id();
    if let Err(e) = write_files(app, &id, rgba, w, h) {
        crate::log!("[shelf] could not store shot: {e}");
        return;
    }
    app.state::<AppState>().shelf.lock().unwrap().insert(0, Item { id, width: w, height: h });
    changed(app);
    // Async spawn, not a bare call: building a window off a plain thread is fine, but this
    // keeps window creation on the same path as async commands (see CLAUDE.md).
    let app = app.clone();
    tauri::async_runtime::spawn(async move { ensure_window(&app) });
}

pub fn list(app: &AppHandle) -> Vec<Item> {
    app.state::<AppState>().shelf.lock().unwrap().clone()
}

fn delete_files(app: &AppHandle, id: &str, after: Duration) {
    let dir = dir(app);
    let id = id.to_string();
    let run = move || {
        let _ = std::fs::remove_file(dir.join(format!("{id}.png")));
        let _ = std::fs::remove_file(dir.join(format!("{id}.thumb.png")));
    };
    if after.is_zero() {
        run();
    } else {
        std::thread::spawn(move || {
            std::thread::sleep(after);
            run();
        });
    }
}

/// `dropped`: the shot was dragged into another app, which may still be reading the file.
pub fn remove(app: &AppHandle, id: &str, dropped: bool) {
    let removed = {
        let state = app.state::<AppState>();
        let mut items = state.shelf.lock().unwrap();
        let before = items.len();
        items.retain(|i| i.id != id);
        items.len() != before
    };
    if removed {
        delete_files(app, id, if dropped { KEEP_AFTER_DROP } else { Duration::ZERO });
        changed(app);
    }
}

pub fn clear(app: &AppHandle) {
    let ids: Vec<String> = std::mem::take(&mut *app.state::<AppState>().shelf.lock().unwrap())
        .into_iter()
        .map(|i| i.id)
        .collect();
    for id in &ids {
        delete_files(app, id, Duration::ZERO);
    }
    changed(app);
}

fn changed(app: &AppHandle) {
    let _ = app.emit_to(LABEL, "shelf:changed", ());
}

/// Path of a file (full PNG or thumbnail) for the page's `shelf://` URLs and for the drag.
pub fn file_for(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
    if !safe_name(name) {
        return Err("bad name".into());
    }
    Ok(dir(app).join(name))
}

/// Only `<id>.png` / `<id>.thumb.png` with a plain id: no path tricks from the page.
fn safe_name(name: &str) -> bool {
    name.ends_with(".png")
        && name.len() < 80
        && !name.contains("..")
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'))
}

/// Create the (hidden) shelf window if it isn't there. Must run on an async task or the main thread.
fn ensure_window(app: &AppHandle) {
    if app.get_webview_window(LABEL).is_some() {
        return;
    }
    let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("shelf.html".into()))
        .title("Sky Shot shelf")
        .inner_size(WIN_W, 200.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .focused(false) // never steal focus from what the user is typing in
        .focusable(false)
        .visible(false) // the page asks for a size, which shows it
        .build();
    match built {
        Ok(win) => {
            let handle = app.clone();
            let generation = std::sync::Arc::new(AtomicU64::new(0));
            win.on_window_event(move |e| {
                // Settle ~250 ms after the last move: that is the user letting go of a drag.
                if let WindowEvent::Moved(_) = e {
                    let n = generation.fetch_add(1, Ordering::SeqCst) + 1;
                    let (handle, generation) = (handle.clone(), generation.clone());
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(250));
                        if generation.load(Ordering::SeqCst) == n {
                            snap(&handle);
                        }
                    });
                }
            });
        }
        Err(e) => crate::log!("[shelf] window failed: {e}"),
    }
}

fn monitor_rect(win: &WebviewWindow) -> Option<(f64, f64, f64, f64)> {
    let mon = win.current_monitor().ok()??;
    let (p, s) = (mon.work_area().position, mon.work_area().size);
    Some((p.x as f64, p.y as f64, s.width as f64, s.height as f64))
}

/// Dock the window to `side` of its monitor, centred vertically. Skips the move if already there
/// (a move would fire another Moved event).
fn dock(win: &WebviewWindow, side: Side) {
    let (Some(mon), Ok(size), Ok(pos)) = (monitor_rect(win), win.outer_size(), win.outer_position()) else {
        return;
    };
    let margin = MARGIN * win.scale_factor().unwrap_or(1.0);
    let (x, y) = dock_pos(side, mon, (size.width as f64, size.height as f64), margin);
    let target = PhysicalPosition::new(x.round() as i32, y.round() as i32);
    if target != pos {
        let _ = win.set_position(target);
    }
}

/// After the user drops the shelf: pick the nearer edge, remember it, dock there.
fn snap(app: &AppHandle) {
    let Some(win) = app.get_webview_window(LABEL) else { return };
    let (Some(mon), Ok(size), Ok(pos)) = (monitor_rect(&win), win.outer_size(), win.outer_position()) else {
        return;
    };
    let side = snap_side(pos.x as f64 + size.width as f64 / 2.0, mon.0, mon.2);
    let state = app.state::<AppState>();
    let mut s = state.settings.lock().unwrap().clone();
    if s.shelf_side != side {
        s.shelf_side = side;
        let _ = settings::save(app, &s);
        *state.settings.lock().unwrap() = s;
    }
    dock(&win, side);
}

/// The page reports how tall its content is (logical px); fit the window to it (up to 80% of the
/// work area), dock it, and show it, or hide it when there is nothing on the shelf.
pub fn fit(app: &AppHandle, height: f64) {
    let Some(win) = app.get_webview_window(LABEL) else { return };
    if list(app).is_empty() {
        let _ = win.hide();
        return;
    }
    let scale = win.scale_factor().unwrap_or(1.0);
    let max = monitor_rect(&win).map_or(f64::MAX, |m| m.3 * 0.8);
    let h = (height * scale).min(max).max(60.0 * scale);
    let _ = win.set_size(PhysicalSize::new((WIN_W * scale).round() as u32, h.round() as u32));
    let side = app.state::<AppState>().settings.lock().unwrap().shelf_side;
    dock(&win, side);
    let _ = win.show(); // non-focusable, so this doesn't activate it
}

/// Turn pinning on or off: persist, update the tray tick, and tell every overlay.
pub fn set_pinning(app: &AppHandle, enabled: bool) {
    let state = app.state::<AppState>();
    {
        let mut s = state.settings.lock().unwrap().clone();
        if s.pinning != enabled {
            s.pinning = enabled;
            let _ = settings::save(app, &s);
            *state.settings.lock().unwrap() = s;
        }
    }
    if let Some(item) = state.pin_item.lock().unwrap().as_ref() {
        let _ = item.set_checked(enabled);
    }
    let _ = app.emit("pinning:changed", enabled);
}

pub fn pinning(app: &AppHandle) -> bool {
    app.state::<AppState>().settings.lock().unwrap().pinning
}

/// `shelf://<id>.thumb.png` (http://shelf.localhost/… on Windows): thumbnails for the page.
pub fn protocol(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    let app = ctx.app_handle().clone();
    std::thread::spawn(move || {
        let name = request.uri().path().trim_start_matches('/').to_string();
        let file = file_for(&app, &name).and_then(|p| std::fs::read(p).map_err(|e| e.to_string()));
        let response = match file {
            Ok(bytes) => tauri::http::Response::builder()
                .header("Content-Type", "image/png")
                .header("Cache-Control", "max-age=31536000, immutable")
                .body(bytes),
            Err(_) => tauri::http::Response::builder().status(404).body(Vec::new()),
        };
        responder.respond(response.unwrap());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_follows_the_nearer_edge() {
        // 1920 px wide monitor starting at 0; the shelf is 176 wide.
        assert_eq!(snap_side(88.0, 0.0, 1920.0), Side::Left);
        assert_eq!(snap_side(959.0, 0.0, 1920.0), Side::Left);
        assert_eq!(snap_side(960.0, 0.0, 1920.0), Side::Left); // exact middle: default side
        assert_eq!(snap_side(961.0, 0.0, 1920.0), Side::Right);
        assert_eq!(snap_side(1830.0, 0.0, 1920.0), Side::Right);
    }

    #[test]
    fn side_works_on_a_second_monitor() {
        // Monitor to the left of the primary has a negative x; one on the right starts at 1920.
        assert_eq!(snap_side(-1800.0, -1920.0, 1920.0), Side::Left);
        assert_eq!(snap_side(-100.0, -1920.0, 1920.0), Side::Right);
        assert_eq!(snap_side(2000.0, 1920.0, 1920.0), Side::Left);
        assert_eq!(snap_side(3800.0, 1920.0, 1920.0), Side::Right);
    }

    #[test]
    fn docking_centres_vertically_against_the_edge() {
        let mon = (0.0, 0.0, 1920.0, 1040.0);
        assert_eq!(dock_pos(Side::Left, mon, (176.0, 400.0), 8.0), (8.0, 320.0));
        assert_eq!(dock_pos(Side::Right, mon, (176.0, 400.0), 8.0), (1736.0, 320.0));
        let second = (1920.0, 100.0, 1280.0, 1000.0);
        assert_eq!(dock_pos(Side::Right, second, (176.0, 500.0), 8.0), (3016.0, 350.0));
    }

    #[test]
    fn thumbnails_keep_aspect_and_cap_height() {
        assert_eq!(thumb_size(1440, 900), (288, 180));
        assert_eq!(thumb_size(100, 50), (100, 50)); // never enlarged
        let (w, h) = thumb_size(1000, 20000); // a long scrolling shot
        assert!(h <= THUMB_MAX_H && w >= 1);
    }

    #[test]
    fn only_plain_png_names_are_served() {
        assert!(safe_name("1700000000000-1.png") && safe_name("1700000000000-1.thumb.png"));
        for bad in ["../x.png", "a/b.png", "a\\b.png", "x.txt", "..png", ""] {
            assert!(!safe_name(bad), "{bad}");
        }
    }
}
