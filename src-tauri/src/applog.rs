//! A small log file in the app data folder (`logs/shutter.log`), so a report like "it blinks
//! sometimes" can be checked on the owner's machine: the release build has no console.
//! Lines are also printed, so `tauri dev` output is unchanged. One older file is kept
//! (`shutter.old.log`); the current one rolls over at 1 MB.

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

const MAX_BYTES: u64 = 1_000_000;

static FILE: OnceLock<Mutex<File>> = OnceLock::new();
static DIR: OnceLock<PathBuf> = OnceLock::new();

/// Open the log in `dir` (rolling the old one over if it's big) and log panics there too.
pub fn init(dir: PathBuf) {
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("shutter.log");
    roll(&path, &dir.join("shutter.old.log"));
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = FILE.set(Mutex::new(f));
        let _ = DIR.set(dir);
    }
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write(&format!("[panic] {info}"));
        default(info);
    }));
    write(&format!("[start] Sky Shot {}", env!("CARGO_PKG_VERSION")));
}

fn roll(path: &Path, old: &Path) {
    if fs::metadata(path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = fs::rename(path, old);
    }
}

pub fn dir() -> Option<&'static PathBuf> {
    DIR.get()
}

/// Print a line and append it, timestamped, to the log file.
pub fn write(line: &str) {
    println!("{line}");
    if let Some(Ok(mut f)) = FILE.get().map(|f| f.lock()) {
        let _ = writeln!(f, "{} {line}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"));
    }
}

/// `log!("[capture] visible {ms} ms")`: like println!, and kept in the log file.
#[macro_export]
macro_rules! log {
    ($($t:tt)*) => { $crate::applog::write(&format!($($t)*)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolls_over_only_when_big() {
        let dir = std::env::temp_dir().join(format!("shutter-log-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (cur, old) = (dir.join("a.log"), dir.join("a.old.log"));
        fs::write(&cur, "small").unwrap();
        roll(&cur, &old);
        assert!(cur.exists() && !old.exists(), "a small log stays");
        fs::write(&cur, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        roll(&cur, &old);
        assert!(!cur.exists() && old.exists(), "a big log becomes the old one");
        let _ = fs::remove_dir_all(&dir);
    }
}
