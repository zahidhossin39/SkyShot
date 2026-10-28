//! Where a finished capture goes: clipboard, disk, or both.

use crate::{settings, state::AppState};
use std::{borrow::Cow, path::PathBuf};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;

pub fn copy_image(rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut c| {
            c.set_image(arboard::ImageData {
                width: width as usize,
                height: height as usize,
                bytes: Cow::Borrowed(rgba),
            })
        })
        .map_err(|e| format!("Clipboard error: {e}"))
}

pub fn copy_text(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text))
        .map_err(|e| format!("Clipboard error: {e}"))
}

fn save_png(path: &PathBuf, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    image::save_buffer(path, rgba, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|e| format!("Could not save: {e}"))
}

fn file_name() -> String {
    chrono::Local::now()
        .format("Screenshot %Y-%m-%d %H%M%S.png")
        .to_string()
}

/// Unique path in the save folder (adds " (2)" etc. if two shots land in the same second).
fn default_path(app: &AppHandle) -> PathBuf {
    let s = app.state::<AppState>().settings.lock().unwrap().clone();
    let dir = settings::save_dir(app, &s);
    let base = file_name();
    let mut path = dir.join(&base);
    let mut n = 2;
    while path.exists() {
        path = dir.join(base.replace(".png", &format!(" ({n}).png")));
        n += 1;
    }
    path
}

pub fn notify(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}

/// action: "copy" | "save" | "both" | "saveas". Returns the saved path, if any.
pub fn finish(
    app: &AppHandle,
    action: &str,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<Option<String>, String> {
    if matches!(action, "copy" | "both") {
        copy_image(rgba, width, height)?;
    }
    let path = match action {
        "save" | "both" => Some(default_path(app)),
        "saveas" => {
            let default = default_path(app);
            let picked = app
                .dialog()
                .file()
                .add_filter("PNG image", &["png"])
                .set_directory(default.parent().unwrap_or(&default))
                .set_file_name(default.file_name().unwrap().to_string_lossy())
                .blocking_save_file();
            match picked.and_then(|p| p.into_path().ok()) {
                Some(p) => Some(p),
                None => return Ok(None), // cancelled
            }
        }
        _ => None,
    };
    if let Some(p) = &path {
        save_png(p, rgba, width, height)?;
        let what = if action == "both" { "Copied and saved" } else { "Saved" };
        notify(app, what, &p.display().to_string());
    }
    Ok(path.map(|p| p.display().to_string()))
}
