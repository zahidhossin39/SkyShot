//! Hand things to the OS: open a folder or link, or show a file in the file manager.
//! Arguments go straight to the program (no shell), so odd paths and URLs stay inert.

use std::{path::Path, process::Command};

fn spawn(cmd: &mut Command) -> Result<(), String> {
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// A folder in the file manager, or a web link in the default browser.
pub fn open(target: &str) -> Result<(), String> {
    #[cfg(windows)]
    let mut cmd = if target.starts_with("http") {
        let mut c = Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", target]);
        c
    } else {
        let mut c = Command::new("explorer");
        c.arg(target);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        c.arg(target);
        c
    };
    #[cfg(target_os = "linux")]
    let mut cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(target);
        c
    };
    spawn(&mut cmd)
}

/// The file manager with this file selected (Linux: its folder, no portable way to select).
pub fn reveal(file: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        // `/select,<path>` must be one argument for explorer to highlight the file.
        spawn(Command::new("explorer").arg(format!("/select,{}", file.display())))
    }
    #[cfg(target_os = "macos")]
    {
        spawn(Command::new("open").arg("-R").arg(file))
    }
    #[cfg(target_os = "linux")]
    {
        open(&file.parent().unwrap_or(file).to_string_lossy())
    }
}
