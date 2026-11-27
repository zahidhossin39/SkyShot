// Updates from GitHub Releases (tauri-plugin-updater). The app reads `latest.json` from the
// newest release; the installer it points to must be signed with our key (pubkey in
// tauri.conf.json), so a tampered download is refused.

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Available {
    version: String,
    notes: Option<String>,
    /// Installed by a package manager (.deb/.rpm): link to the download page instead.
    manual: bool,
}

/// Where a manual update comes from.
pub const RELEASES: &str = "https://github.com/zahidhossin39/SkyShot/releases/latest";

/// On Linux only the AppImage can replace itself; .deb/.rpm installs belong to the package
/// manager, and overwriting them would fight it.
fn self_updating() -> bool {
    !cfg!(target_os = "linux") || std::env::var_os("APPIMAGE").is_some()
}

/// The newer version, or None if this one is the latest.
#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<Available>, String> {
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    Ok(update.map(|u| Available { version: u.version, notes: u.body, manual: !self_updating() }))
}

/// Downloads and runs the new installer, then restarts into the new version.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    if !self_updating() {
        return crate::os::open(RELEASES);
    }
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    let Some(update) = update else { return Err("Already up to date".into()) };
    update.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    app.restart();
}

/// Quiet check a little after start; a notification only when there is something new.
pub fn check_on_start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio_sleep().await;
        let found = check_update(app.clone()).await;
        crate::log!("[update] start check: {:?}", found.as_ref().map(|u| u.as_ref().map(|u| &u.version)));
        if let Ok(Some(u)) = found {
            crate::output::notify(
                &app,
                &format!("Sky Shot {} is available", u.version),
                if u.manual {
                    "Download it from the Sky Shot releases page (Settings → Settings has the link)."
                } else {
                    "Open Settings (tray icon) → Settings to update."
                },
            );
        }
    });
}

async fn tokio_sleep() {
    // Off the startup path: hotkeys and the overlay warm-up come first.
    let _ = tauri::async_runtime::spawn_blocking(|| std::thread::sleep(std::time::Duration::from_secs(3))).await;
}
