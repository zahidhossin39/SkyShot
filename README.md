# Sky Shot

_Formerly Shutter._

A fast screenshot tool for Windows, macOS and Linux, built with Tauri 2 (Rust core, TypeScript UI).

## Install

| System | One command |
|---|---|
| **Windows 10/11** | Download `Sky.Shot_x.y.z_x64-setup.exe` from [Releases](https://github.com/zahidhossin39/sky-shot/releases/latest) and run it |
| **macOS 10.15+** (Apple Silicon and Intel) | `brew install --cask zahidhossin39/tap/sky-shot` |
| **macOS or Linux** | `curl -fsSL https://raw.githubusercontent.com/zahidhossin39/sky-shot/main/install.sh \| sh` |

The script picks the right package: `.deb` on Debian, Ubuntu, Mint and Pop!_OS; `.rpm` on Fedora and openSUSE; the self-updating AppImage everywhere else (Arch, immutable distros). Files are also on the [Releases](https://github.com/zahidhossin39/sky-shot/releases/latest) page.

Sky Shot updates itself on Windows, macOS and the AppImage (Settings → Check for updates). `.deb`/`.rpm` installs: run the script again.

**macOS:** the first capture asks for Screen Recording permission (System Settings → Privacy & Security). Sky Shot isn't notarized yet: if you open the `.dmg` by hand, macOS blocks it once; go to Privacy & Security and click **Open Anyway**. Brew and the script skip that step.

**Linux:** the command is `sky-shot`. On Wayland, apps can't register global hotkeys: add a shortcut in your desktop's keyboard settings that runs `sky-shot --capture`. GNOME asks once whether Sky Shot may take screenshots. The tray icon on GNOME needs the AppIndicator extension.

## Features

- **Capture:** Ctrl+PrtSc freezes the screen, dimmed until you drag a selection. Hover a window to snap to it (Windows/macOS).
- **Blur / redact:** drag over anything private and it's blurred for good. Smart Redact finds emails, keys and card numbers.
- **Text grab (OCR), offline:** Ctrl+Shift+PrtSc copies the text in a selection (Windows OCR, Apple Vision, or ocrs on Linux). Live Text with smart actions.
- **Annotate:** pen, arrows, boxes, highlighter, text, numbered steps, 12 colours plus a picker.
- **Pinning shelf:** turn on Pinning and every shot lands on a small shelf at the screen edge; drag one into any app.
- **More:** scrolling capture, zoom loupe and colour picker, searchable History, AI re-read, tray icon, hotkey rebinding, autostart, light/dark theme.

## Run from source

Needs [Node.js](https://nodejs.org) and [Rust](https://rustup.rs), plus the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your system. Linux: run `sh scripts/get-ocr-models.sh` once.

```powershell
npm install
npm run tauri dev
```

Build an installer: `npm run tauri build`.

Tests: `npm test` (JS) and `cargo test` in `src-tauri`.

## License

[MIT](LICENSE)
