//! Pin to screen: float a capture in a small always-on-top window, as a reference.
//! Rust keeps the pixels (for Ctrl+C) and hands the page a PNG; the page does move/zoom/close.

use crate::state::{AppState, Frame};
use std::sync::atomic::{AtomicU32, Ordering};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};

static NEXT: AtomicU32 = AtomicU32::new(0);

/// `x, y`: the selection's top-left inside `overlay`, so the pin appears exactly where it was taken.
pub fn open(app: &AppHandle, overlay: &WebviewWindow, frame: Frame, x: i32, y: i32) -> Result<(), String> {
    let origin = overlay.inner_position().map_err(|e| e.to_string())?; // where the frozen pixels start
    let label = format!("pin-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let size = PhysicalSize::new(frame.width, frame.height);
    app.state::<AppState>().pins.lock().unwrap().insert(label.clone(), frame);
    let win = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("pin.html".into()))
        .title("Pinned screenshot")
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false) // the page shows it once the image is drawn, so there's no white flash
        .build()
        .map_err(|e| e.to_string())?;
    // Position first: moving to another monitor can change the scale factor.
    let target = PhysicalPosition::new(origin.x + x, origin.y + y);
    let _ = win.set_position(target);
    let _ = win.set_size(size);
    // set_position moves the outer frame, and undecorated windows keep an invisible border
    // (11 px sides / 2 px top at 150%). Shift by it so the image lands exactly where it was.
    if let (Ok(o), Ok(i)) = (win.outer_position(), win.inner_position()) {
        let _ = win.set_position(PhysicalPosition::new(target.x - (i.x - o.x), target.y - (i.y - o.y)));
    }
    let app = app.clone();
    win.on_window_event(move |e| {
        if let WindowEvent::Destroyed = e {
            app.state::<AppState>().pins.lock().unwrap().remove(&label);
        }
    });
    Ok(())
}

pub fn png(app: &AppHandle, label: &str) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let state = app.state::<AppState>();
    let pins = state.pins.lock().unwrap();
    let f = pins.get(label).ok_or("pin not found")?;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&f.rgba, f.width, f.height, image::ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

pub fn copy(app: &AppHandle, label: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let pins = state.pins.lock().unwrap();
    let f = pins.get(label).ok_or("pin not found")?;
    crate::output::copy_image(&f.rgba, f.width, f.height)
}
