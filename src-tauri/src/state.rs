use crate::{settings::Settings, Mode};
use std::{
    collections::HashMap,
    sync::{atomic::AtomicBool, Mutex},
    time::Instant,
};
use tauri::{menu::MenuItem, Wry};

/// One monitor's frozen screenshot, waiting to be pulled by its overlay window.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// An open capture: from hotkey press until the overlay closes.
pub struct Session {
    pub mode: Mode,
    pub started: Instant,
}

pub struct AppState {
    pub settings: Mutex<Settings>,
    /// Overlay window label -> frame not yet sent to it.
    pub frames: Mutex<HashMap<String, Frame>>,
    pub session: Mutex<Option<Session>>,
    /// True while the settings window is recording a new hotkey.
    pub hotkeys_paused: AtomicBool,
    pub tray_items: Mutex<Option<(MenuItem<Wry>, MenuItem<Wry>)>>,
}
