//! Searchable history: every finished capture, saved exactly as exported (redactions burned
//! in, so text you hid is never on disk or in the index), with its OCR text for search.
//!
//! Layout in the app data dir: `history/<id>.png`, `history/<id>.thumb.png`, `history/index.json`.
//! One JSON index is plenty for thousands of shots and needs no database.

use crate::{ocr, state::AppState};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::Mutex,
};
use tauri::{AppHandle, Manager};

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    /// Unix time, ms.
    pub created: i64,
    pub width: u32,
    pub height: u32,
    pub text: String,
    pub starred: bool,
}

/// Index read-modify-write must not interleave (saves run on background threads).
static INDEX: Mutex<()> = Mutex::new(());

pub fn dir(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir().expect("no app data dir").join("history")
}

fn load(app: &AppHandle) -> Vec<Entry> {
    fs::read_to_string(dir(app).join("index.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn store(app: &AppHandle, entries: &[Entry]) -> Result<(), String> {
    let path = dir(app).join("index.json");
    let tmp = path.with_extension("json.tmp");
    // Write then rename, so a crash mid-write can't wipe the index.
    fs::write(&tmp, serde_json::to_string(entries).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// Ids come back from the page and become file names, so only allow what `add` makes.
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn retention_days(app: &AppHandle) -> u32 {
    app.state::<AppState>().settings.lock().unwrap().history_days
}

/// Ids past retention (`days` 0 = forever). Starred shots are always kept.
fn expired(entries: &[Entry], now_ms: i64, days: u32) -> Vec<String> {
    if days == 0 {
        return vec![];
    }
    let cutoff = now_ms - days as i64 * 86_400_000;
    entries.iter().filter(|e| !e.starred && e.created < cutoff).map(|e| e.id.clone()).collect()
}

fn remove_files(app: &AppHandle, id: &str) {
    let d = dir(app);
    let _ = fs::remove_file(d.join(format!("{id}.png")));
    let _ = fs::remove_file(d.join(format!("{id}.thumb.png")));
}

/// Save one capture. Runs OCR, so call it off the UI path.
pub fn add(app: &AppHandle, rgba: Vec<u8>, width: u32, height: u32) -> Result<(), String> {
    let t = std::time::Instant::now();
    let now = chrono::Local::now();
    let id = now.format("%Y%m%d-%H%M%S-%3f").to_string();
    let d = dir(app);
    fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let img = image::RgbaImage::from_raw(width, height, rgba).ok_or("bad image size")?;
    img.save(d.join(format!("{id}.png"))).map_err(|e| e.to_string())?;
    // Fit in 480x320 for the grid (2x its CSS size, so it stays sharp at 200% scaling).
    let k = (480.0 / width as f32).min(320.0 / height as f32).min(1.0);
    image::imageops::thumbnail(&img, ((width as f32 * k) as u32).max(1), ((height as f32 * k) as u32).max(1))
        .save(d.join(format!("{id}.thumb.png")))
        .map_err(|e| e.to_string())?;
    let text = ocr::recognize(img.as_raw(), width, height).map(|o| o.text).unwrap_or_default();

    let _lock = INDEX.lock().unwrap();
    let mut entries = load(app);
    entries.push(Entry { id, created: now.timestamp_millis(), width, height, text, starred: false });
    store(app, &entries)?;
    drop(_lock);
    prune(app);
    println!("[history] saved {width}x{height} in {} ms", t.elapsed().as_millis());
    Ok(())
}

/// Apply the retention setting to everything (also called when the setting changes).
pub fn prune(app: &AppHandle) {
    let _lock = INDEX.lock().unwrap();
    let mut entries = load(app);
    let gone = expired(&entries, chrono::Local::now().timestamp_millis(), retention_days(app));
    if gone.is_empty() {
        return;
    }
    gone.iter().for_each(|id| remove_files(app, id));
    entries.retain(|e| !gone.contains(&e.id));
    let _ = store(app, &entries);
}

/// Newest first.
pub fn list(app: &AppHandle) -> Vec<Entry> {
    let _lock = INDEX.lock().unwrap();
    let mut entries = load(app);
    entries.reverse();
    entries
}

pub fn set_star(app: &AppHandle, id: &str, starred: bool) -> Result<(), String> {
    let _lock = INDEX.lock().unwrap();
    let mut entries = load(app);
    let e = entries.iter_mut().find(|e| e.id == id).ok_or("not found")?;
    e.starred = starred;
    store(app, &entries)
}

pub fn delete(app: &AppHandle, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err("bad id".into());
    }
    let _lock = INDEX.lock().unwrap();
    let mut entries = load(app);
    entries.retain(|e| e.id != id);
    remove_files(app, id);
    store(app, &entries)
}

/// Full-size image path for an entry (checked, so it can't point outside the history dir).
pub fn image_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("bad id".into());
    }
    let p = dir(app).join(format!("{id}.png"));
    p.exists().then_some(p).ok_or_else(|| "file missing".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, days_old: i64, starred: bool) -> Entry {
        Entry { id: id.into(), created: 100 * 86_400_000 - days_old * 86_400_000, width: 1, height: 1, text: String::new(), starred }
    }

    #[test]
    fn retention_drops_old_unstarred_only() {
        let now = 100 * 86_400_000;
        let es = [entry("new", 1, false), entry("old", 40, false), entry("old-star", 40, true)];
        assert_eq!(expired(&es, now, 30), vec!["old".to_string()]);
        assert!(expired(&es, now, 0).is_empty(), "0 = keep forever");
        assert_eq!(expired(&es, now, 1).len(), 1, "exactly 1 day old is kept");
    }

    #[test]
    fn ids_cannot_escape_the_folder() {
        assert!(valid_id("20261029-143005-123"));
        for bad in ["", "..\\settings", "../x", "a/b", "C:x", "a.png"] {
            assert!(!valid_id(bad), "{bad}");
        }
    }
}
