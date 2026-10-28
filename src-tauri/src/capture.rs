//! Capture flow: hotkey -> grab every monitor -> show one overlay window per monitor.
//!
//! Overlay windows are created once and kept hidden ("warm"), so a capture only has to
//! grab pixels and hand them over. That's what keeps hotkey -> visible fast.

use crate::{
    state::{AppState, Frame, Session},
    Mode,
};
use serde::Serialize;
use std::time::Instant;
use tauri::{
    AppHandle, Emitter, EventTarget, Manager, PhysicalPosition, PhysicalSize, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, window::Color,
};

const PREFIX: &str = "overlay-";

#[derive(Clone, Serialize)]
pub struct OpenPayload {
    pub mode: Mode,
    pub width: u32,
    pub height: u32,
}

fn overlay_windows(app: &AppHandle) -> Vec<WebviewWindow> {
    app.webview_windows()
        .into_iter()
        .filter(|(label, _)| label.starts_with(PREFIX))
        .map(|(_, w)| w)
        .collect()
}

fn get_or_create(app: &AppHandle, label: &str) -> Result<WebviewWindow, String> {
    if let Some(w) = app.get_webview_window(label) {
        return Ok(w);
    }
    WebviewWindowBuilder::new(app, label, WebviewUrl::App("overlay.html".into()))
        .title("Screenshot overlay")
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .visible(false)
        .focused(false)
        .background_color(Color(0, 0, 0, 255))
        .build()
        .map_err(|e| e.to_string())
}

/// Create one hidden overlay per monitor at startup so the first capture is fast too.
pub fn warm_up(app: &AppHandle) {
    let count = xcap::Monitor::all().map(|m| m.len()).unwrap_or(1);
    for i in 0..count {
        if let Err(e) = get_or_create(app, &format!("{PREFIX}{i}")) {
            eprintln!("[capture] warm-up failed: {e}");
        }
    }
}

pub fn start(app: &AppHandle, mode: Mode) {
    let state = app.state::<AppState>();
    {
        let mut session = state.session.lock().unwrap();
        if session.is_some() {
            return; // already capturing
        }
        *session = Some(Session { mode, started: Instant::now() });
    }
    // Capture off the main thread so the event loop stays responsive.
    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = open(&app, mode) {
            eprintln!("[capture] failed: {e}");
            close(&app);
        }
    });
}

fn open(app: &AppHandle, mode: Mode) -> Result<(), String> {
    let state = app.state::<AppState>();
    let t0 = Instant::now();
    let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
    let mut shots = Vec::with_capacity(monitors.len());
    for m in &monitors {
        let x = m.x().map_err(|e| e.to_string())?;
        let y = m.y().map_err(|e| e.to_string())?;
        let img = m.capture_image().map_err(|e| e.to_string())?;
        shots.push((x, y, img));
    }
    let capture_ms = t0.elapsed().as_millis();
    let bounds: Vec<_> = shots.iter().map(|(x, y, img)| (*x, *y, img.width(), img.height())).collect();

    for (i, (x, y, img)) in shots.into_iter().enumerate() {
        let label = format!("{PREFIX}{i}");
        let win = get_or_create(app, &label)?;
        let (width, height) = img.dimensions();
        // Position first: moving to another monitor can change the scale factor.
        let _ = win.set_position(PhysicalPosition::new(x, y));
        let _ = win.set_size(PhysicalSize::new(width, height));
        state.frames.lock().unwrap().insert(
            label.clone(),
            Frame { width, height, rgba: img.into_raw() },
        );
        app.emit_to(
            EventTarget::webview_window(&label),
            "overlay:open",
            OpenPayload { mode, width, height },
        )
        .map_err(|e| e.to_string())?;
    }
    // Hide overlays for monitors that were unplugged since last time.
    for w in overlay_windows(app) {
        let idx: usize = w.label()[PREFIX.len()..].parse().unwrap_or(usize::MAX);
        if idx >= monitors.len() {
            let _ = w.hide();
        }
    }
    println!(
        "[capture] grabbed {} monitor(s) in {capture_ms} ms, events sent at {} ms",
        monitors.len(),
        since_hotkey(app)
    );
    // Window rects for snap-to-window. Sent after the frames, so they never slow the hotkey.
    let app = app.clone();
    std::thread::spawn(move || send_windows(&app, &bounds));
    Ok(())
}

/// Each overlay gets the visible windows on its monitor as `[x, y, w, h]` (monitor-relative
/// physical px, clipped to the monitor), top-most first, so hovering can outline one.
fn send_windows(app: &AppHandle, monitors: &[(i32, i32, u32, u32)]) {
    let t = Instant::now();
    let me = std::process::id();
    // List order is z-order. Skip title/app_name (slow) and our own overlays.
    let wins: Vec<(i32, i32, i32, i32)> = xcap::Window::all()
        .unwrap_or_default()
        .iter()
        .filter_map(|w| {
            if w.pid().ok()? == me || w.is_minimized().ok()? {
                return None;
            }
            let (x, y) = (w.x().ok()?, w.y().ok()?);
            Some((x, y, x + w.width().ok()? as i32, y + w.height().ok()? as i32))
        })
        .collect();
    for (i, &(mx, my, mw, mh)) in monitors.iter().enumerate() {
        let rects = clip_to_monitor(&wins, (mx, my, mw, mh));
        let _ = app.emit_to(EventTarget::webview_window(format!("{PREFIX}{i}")), "overlay:windows", rects);
    }
    println!("[capture] {} window rects in {} ms", wins.len(), t.elapsed().as_millis());
}

/// Window edges (left, top, right, bottom) -> `[x, y, w, h]` inside one monitor. Drops slivers
/// and the tiny hidden helper windows some apps park at (0, 0) (22x22 seen in the wild).
fn clip_to_monitor(wins: &[(i32, i32, i32, i32)], (mx, my, mw, mh): (i32, i32, u32, u32)) -> Vec<[i32; 4]> {
    wins.iter()
        .filter_map(|&(l, t, r, b)| {
            let (l, t) = (l.max(mx), t.max(my));
            let (r, b) = (r.min(mx + mw as i32), b.min(my + mh as i32));
            (r - l >= 40 && b - t >= 40).then_some([l - mx, t - my, r - l, b - t])
        })
        .collect()
}

#[cfg(test)]
#[test]
fn windows_clip_to_their_monitor() {
    // Second monitor at x=1920. A window straddling both, one off it, one too thin, one tiny.
    let wins = [(1800, 100, 2200, 500), (0, 0, 800, 600), (2000, 0, 2010, 300), (1920, 0, 1942, 22)];
    assert_eq!(clip_to_monitor(&wins, (1920, 0, 1920, 1080)), vec![[0, 100, 280, 400]]);
}

/// Overlay asks on load whether a frame is already waiting (covers a window that was
/// created after the open event fired).
pub fn pending(app: &AppHandle, label: &str) -> Option<OpenPayload> {
    let state = app.state::<AppState>();
    let mode = state.session.lock().unwrap().as_ref()?.mode;
    let frames = state.frames.lock().unwrap();
    let f = frames.get(label)?;
    Some(OpenPayload { mode, width: f.width, height: f.height })
}

pub fn take_frame(app: &AppHandle, label: &str) -> Option<Frame> {
    app.state::<AppState>().frames.lock().unwrap().remove(label)
}

/// Hand raw RGBA to the page through WebView2 shared memory. Pushing 8+ MB through
/// Tauri IPC costs ~100 ms; this is one memcpy. The page gets a `sharedbufferreceived` event.
#[cfg(windows)]
pub fn post_frame(wv: &tauri::webview::PlatformWebview, rgba: &[u8]) -> Result<(), String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Environment12, ICoreWebView2_17, COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY,
    };
    use windows::core::{Interface, PCWSTR};
    let err = |e: windows::core::Error| e.to_string();    unsafe {
        let env: ICoreWebView2Environment12 = wv.environment().cast().map_err(err)?;
        let core: ICoreWebView2_17 = wv.controller().CoreWebView2().map_err(err)?.cast().map_err(err)?;
        let buf = env.CreateSharedBuffer(rgba.len() as u64).map_err(err)?;
        let mut ptr = std::ptr::null_mut();
        buf.Buffer(&mut ptr).map_err(err)?;
        std::ptr::copy_nonoverlapping(rgba.as_ptr(), ptr, rgba.len());
        core.PostSharedBufferToScript(&buf, COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY, PCWSTR::null())
            .map_err(err)
    }
}

#[cfg(not(windows))]
pub fn post_frame(_wv: &tauri::webview::PlatformWebview, _rgba: &[u8]) -> Result<(), String> {
    Err("shared-buffer frames are Windows-only for now".into())
}

/// Overlay has drawn its frame: show it. Focus goes to the monitor under the cursor.
pub fn shown(app: &AppHandle, win: &WebviewWindow) -> u128 {
    let _ = win.show();
    let under_cursor = (|| {
        let c = app.cursor_position().ok()?;
        let p = win.outer_position().ok()?;
        let s = win.outer_size().ok()?;
        Some(
            c.x >= p.x as f64
                && c.x < (p.x + s.width as i32) as f64
                && c.y >= p.y as f64
                && c.y < (p.y + s.height as i32) as f64,
        )
    })()
    .unwrap_or(true);
    if under_cursor {
        let _ = win.set_focus();
    }
    let ms = since_hotkey(app);
    println!("[capture] {} visible {ms} ms after hotkey", win.label());
    ms
}

/// Milliseconds since the hotkey that started the current session (0 if none).
pub fn since_hotkey(app: &AppHandle) -> u128 {
    app.state::<AppState>()
        .session
        .lock()
        .unwrap()
        .as_ref()
        .map(|s| s.started.elapsed().as_millis())
        .unwrap_or(0)
}

pub fn close(app: &AppHandle) {
    let state = app.state::<AppState>();
    *state.session.lock().unwrap() = None;
    state.frames.lock().unwrap().clear();
    for w in overlay_windows(app) {
        let _ = app.emit_to(EventTarget::webview_window(w.label()), "overlay:reset", ());
        let _ = w.hide();
    }
}
