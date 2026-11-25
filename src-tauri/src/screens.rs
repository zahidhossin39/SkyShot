//! Monitors and their pixels, one API for every OS.
//!
//! Windows and macOS: the `xcap` crate. Linux: our own, because xcap's Linux side links
//! PipeWire bindings that need newer system headers than Ubuntu 22.04 / Debian 12 have, and
//! we build on the oldest glibc we support. X11 reads the root window; Wayland asks the
//! xdg-desktop-portal for one screenshot of the whole desktop (apps can't read the screen).

use image::RgbaImage;
use tauri::AppHandle;

/// A monitor in the units the OS positions windows and reports window rects in: physical
/// pixels on Windows and Linux (scale 1), points on macOS (scale = image pixels per point).
#[derive(Clone, Copy, Debug)]
pub struct Screen {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub scale: f64,
}

// ---------------------------------------------------------------- Windows / macOS (xcap)

#[cfg(not(target_os = "linux"))]
fn screen_of(m: &xcap::Monitor, img: &RgbaImage) -> Result<Screen, String> {
    let s = |e: xcap::XCapError| e.to_string();
    let (x, y) = (m.x().map_err(s)?, m.y().map_err(s)?);
    if cfg!(target_os = "macos") {
        let w = m.width().map_err(s)?.max(1);
        Ok(Screen { x, y, w, h: m.height().map_err(s)?, scale: img.width() as f64 / w as f64 })
    } else {
        Ok(Screen { x, y, w: img.width(), h: img.height(), scale: 1.0 })
    }
}

#[cfg(not(target_os = "linux"))]
pub fn count(_: &AppHandle) -> usize {
    xcap::Monitor::all().map(|m| m.len()).unwrap_or(1)
}

/// Every monitor with a fresh capture, in a stable order (overlay N = monitor N).
#[cfg(not(target_os = "linux"))]
pub fn capture_all(_: &AppHandle) -> Result<Vec<(Screen, RgbaImage)>, String> {
    let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
    monitors
        .iter()
        .map(|m| {
            let img = m.capture_image().map_err(|e| e.to_string())?;
            Ok((screen_of(m, &img)?, img))
        })
        .collect()
}

/// Monitor `idx` and a function that grabs it again (scrolling capture calls it per step).
#[cfg(not(target_os = "linux"))]
pub fn grabber(_: &AppHandle, idx: usize) -> Result<(Screen, impl Fn() -> Result<RgbaImage, String>), String> {
    let m = xcap::Monitor::all().map_err(|e| e.to_string())?.into_iter().nth(idx).ok_or("monitor gone")?;
    let img = m.capture_image().map_err(|e| e.to_string())?;
    let screen = screen_of(&m, &img)?;
    Ok((screen, move || m.capture_image().map_err(|e| e.to_string())))
}

/// Visible windows as (left, top, right, bottom) in OS units, top-most first, minus ours.
#[cfg(not(target_os = "linux"))]
pub fn windows() -> Vec<(i32, i32, i32, i32)> {
    let me = std::process::id();
    // Skip title/app_name (slow).
    xcap::Window::all()
        .unwrap_or_default()
        .iter()
        .filter_map(|w| {
            if w.pid().ok()? == me || w.is_minimized().ok()? {
                return None;
            }
            let (x, y) = (w.x().ok()?, w.y().ok()?);
            Some((x, y, x + w.width().ok()? as i32, y + w.height().ok()? as i32))
        })
        .collect()
}

// ---------------------------------------------------------------- Linux

#[cfg(target_os = "linux")]
pub fn wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland")
}

/// Monitor layout from Tauri (GDK), which knows it on X11 and Wayland alike.
#[cfg(target_os = "linux")]
fn monitors(app: &AppHandle) -> Result<Vec<Screen>, String> {
    let list = app.available_monitors().map_err(|e| e.to_string())?;
    if list.is_empty() {
        return Err("no monitors found".into());
    }
    Ok(list
        .iter()
        .map(|m| Screen {
            x: m.position().x,
            y: m.position().y,
            w: m.size().width,
            h: m.size().height,
            scale: 1.0,
        })
        .collect())
}

#[cfg(target_os = "linux")]
pub fn count(app: &AppHandle) -> usize {
    monitors(app).map(|m| m.len()).unwrap_or(1)
}

#[cfg(target_os = "linux")]
pub fn capture_all(app: &AppHandle) -> Result<Vec<(Screen, RgbaImage)>, String> {
    let screens = monitors(app)?;
    if wayland() {
        let desktop = portal_screenshot()?;
        return Ok(screens.iter().map(|s| (*s, crop_desktop(&desktop, &screens, s))).collect());
    }
    let x = X11::connect()?;
    screens.iter().map(|s| Ok((*s, x.grab(s)?))).collect()
}

#[cfg(target_os = "linux")]
pub fn grabber(app: &AppHandle, idx: usize) -> Result<(Screen, impl Fn() -> Result<RgbaImage, String>), String> {
    if wayland() {
        return Err("Scrolling capture isn't available on Wayland: apps can't scroll other windows there.".into());
    }
    let s = *monitors(app)?.get(idx).ok_or("monitor gone")?;
    let x = X11::connect()?;
    Ok((s, move || x.grab(&s)))
}

/// Snap-to-window on Linux: not yet (X11 would need _NET_CLIENT_LIST_STACKING; Wayland
/// doesn't say where windows are). Selections still work, just without the outline.
#[cfg(target_os = "linux")]
pub fn windows() -> Vec<(i32, i32, i32, i32)> {
    Vec::new()
}

/// The portal gives one image of the whole desktop; cut out one monitor. Its pixel size
/// can differ from the layout (fractional scaling), so map through the layout's bounds.
#[cfg(target_os = "linux")]
fn crop_desktop(desktop: &RgbaImage, all: &[Screen], s: &Screen) -> RgbaImage {
    let left = all.iter().map(|m| m.x).min().unwrap_or(0);
    let top = all.iter().map(|m| m.y).min().unwrap_or(0);
    let right = all.iter().map(|m| m.x + m.w as i32).max().unwrap_or(1);
    let k = desktop.width() as f64 / (right - left).max(1) as f64;
    let px = |v: f64| (v * k).round().max(0.0) as u32;
    let (x, y) = (px((s.x - left) as f64), px((s.y - top) as f64));
    let w = px(s.w as f64).min(desktop.width().saturating_sub(x)).max(1);
    let h = px(s.h as f64).min(desktop.height().saturating_sub(y)).max(1);
    image::imageops::crop_imm(desktop, x, y, w, h).to_image()
}

#[cfg(target_os = "linux")]
struct X11 {
    conn: x11rb::rust_connection::RustConnection,
    root: u32,
}

#[cfg(target_os = "linux")]
impl X11 {
    fn connect() -> Result<Self, String> {
        use x11rb::connection::Connection;
        let (conn, n) = x11rb::connect(None).map_err(|e| format!("Couldn't reach the X server: {e}"))?;
        let root = conn.setup().roots[n].root;
        Ok(X11 { conn, root })
    }

    /// One monitor's pixels from the root window (ZPixmap, 32 bits per pixel: B, G, R, X).
    fn grab(&self, s: &Screen) -> Result<RgbaImage, String> {
        use x11rb::protocol::xproto::{ConnectionExt, ImageFormat};
        let err = |e: &dyn std::fmt::Display| format!("X11 capture failed: {e}");
        let reply = self
            .conn
            .get_image(ImageFormat::Z_PIXMAP, self.root, s.x as i16, s.y as i16, s.w as u16, s.h as u16, !0)
            .map_err(|e| err(&e))?
            .reply()
            .map_err(|e| err(&e))?;
        let mut px = reply.data;
        if px.len() != (s.w * s.h * 4) as usize {
            return Err(format!("X11 capture: unsupported pixel format (depth {})", reply.depth));
        }
        for p in px.as_chunks_mut::<4>().0 {
            p.swap(0, 2); // BGRX -> RGBX
            p[3] = 255;
        }
        RgbaImage::from_raw(s.w, s.h, px).ok_or_else(|| "X11 capture: bad size".into())
    }
}

/// org.freedesktop.portal.Screenshot, non-interactive. GNOME asks the user once whether
/// Shutter may take screenshots; KDE and wlroots portals just answer.
#[cfg(target_os = "linux")]
fn portal_screenshot() -> Result<RgbaImage, String> {
    use std::collections::HashMap;
    use zbus::zvariant::{OwnedValue, Value};
    let err = |e: zbus::Error| format!("Screenshot portal: {e}");
    let conn = zbus::blocking::Connection::session().map_err(err)?;
    // Subscribe to the reply before asking, on the path the portal will use for it.
    let token = format!("shutter{}", std::process::id() ^ chrono::Local::now().timestamp_subsec_nanos());
    let sender = conn.unique_name().ok_or("Screenshot portal: no bus name")?.trim_start_matches(':').replace('.', "_");
    let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    let request = zbus::blocking::Proxy::new(&conn, "org.freedesktop.portal.Desktop", path, "org.freedesktop.portal.Request")
        .map_err(err)?;
    let mut replies = request.receive_signal("Response").map_err(err)?;
    let portal = zbus::blocking::Proxy::new(
        &conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Screenshot",
    )
    .map_err(err)?;
    let mut options: HashMap<&str, Value> = HashMap::new();
    options.insert("handle_token", Value::from(token.as_str()));
    options.insert("interactive", Value::from(false));
    let _: zbus::zvariant::OwnedObjectPath = portal.call("Screenshot", &("", options)).map_err(err)?;

    let reply = replies.next().ok_or("Screenshot portal: no answer")?;
    let (code, results): (u32, HashMap<String, OwnedValue>) = reply.body().deserialize().map_err(err)?;
    if code != 0 {
        return Err("Screenshot permission was denied or cancelled.".into());
    }
    let uri: String = results
        .get("uri")
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| String::try_from(v).ok())
        .ok_or("Screenshot portal: no image")?;
    let file = url_path(&uri).ok_or("Screenshot portal: odd file location")?;
    let img = image::open(&file).map_err(|e| format!("Screenshot portal: {e}"))?.to_rgba8();
    let _ = std::fs::remove_file(&file); // the portal saved it to disk for us; don't leave it
    Ok(img)
}

/// file:///home/me/Pictures/Screenshot%20x.png -> /home/me/Pictures/Screenshot x.png
#[cfg(target_os = "linux")]
fn url_path(uri: &str) -> Option<std::path::PathBuf> {
    let raw = uri.strip_prefix("file://")?;
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(std::path::PathBuf::from(String::from_utf8(out).ok()?))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn portal_uris_become_paths() {
        assert_eq!(url_path("file:///home/a/Pictures/Screenshot%20x.png").unwrap().to_str(), Some("/home/a/Pictures/Screenshot x.png"));
        assert!(url_path("https://x").is_none());
    }

    #[test]
    fn desktop_crop_maps_through_the_layout() {
        // Two 1000x500 monitors side by side; the portal image is half size (scaled desktop).
        let desk = RgbaImage::new(1000, 250);
        let a = Screen { x: 0, y: 0, w: 1000, h: 500, scale: 1.0 };
        let b = Screen { x: 1000, y: 0, w: 1000, h: 500, scale: 1.0 };
        let img = crop_desktop(&desk, &[a, b], &b);
        assert_eq!(img.dimensions(), (500, 250));
    }
}
