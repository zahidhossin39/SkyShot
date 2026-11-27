# Changelog

All notable changes to **Sky Shot** (formerly **Shutter**) are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.8] - 2026-11-27

### Added
- New **Sky Shot** app icon (`src-tauri/icons/app-icon.png`: sky-to-amber gradient with 5 white stars) and Settings brand mark (`src/assets/logo.png`).
- Transparent 5-star HiDPI system tray icon (`src-tauri/icons/tray.png`, 64×64 cropped tight) embedded via `tauri::include_image!` with `build.rs` icon change tracking.

### Changed
- Refreshed **Settings**, **History**, **Shelf**, and **Overlay** with **Material Design 3** tokens (`src/theme.css`: semantic color roles, tonal surfaces, typography and shape scales, and state layers) across both light and dark themes.

## [0.4.1] - 2026-11-27

### Changed
- Dropped the dash across naming: repository `zahidhossin39/SkyShot`, window titles (`Sky Shot Settings` and `Sky Shot History`), Linux package and command `skyshot` (replacing `sky-shot`), and Homebrew cask `skyshot`.

## [0.4.0] - 2026-11-27

### Changed
- Renamed the application from **Shutter** to **Sky Shot** across product name, window titles, tray menu, and installers while keeping the app identifier (`com.zaid-hossain.screenshot-app`) so existing settings, History, and saved AI keys carry over.
- Windows NSIS installer post-install hook (`src-tauri/windows/hooks.nsh`) replaces legacy Shutter installs and migrates the autostart entry automatically.
- Linux packages (`deb`/`rpm`) replace/obsolete `shutter-app`.

## [0.3.2] - 2026-11-27

### Added
- **Pinning shelf** toggle switch in Settings (off by default).

### Changed
- Window title bars in **Settings** and **History** now follow the active light/dark theme.

## [0.3.1] - 2026-11-26

### Fixed
- **Start with Windows** autostart now re-points to the real installed executable on launch so updates are always picked up.

### Changed
- Updated `scripts/release.ps1` with `-Resume` support and draft `latest.json` download-link rewriting.
- Updated `install.sh` to read `latest.json` directly (avoiding GitHub API rate limits) and install the universal `.app` bundle on macOS.

## [0.3.0] - 2026-11-26

### Added
- **macOS** (universal Apple Silicon + Intel) and **Linux** (X11 + Wayland; AppImage, `.deb`, `.rpm`) support:
  - `frame://` overlay protocol for non-Windows platforms.
  - Native Apple Vision OCR on macOS and bundled `ocrs` ONNX models on Linux.
  - Cross-platform AI re-read via `ureq` and OS keyring (macOS Keychain / Linux Secret Service).
  - Scrolling capture input via `enigo` on macOS and Linux X11.
- **Pinning shelf** (`F` toolbar toggle and tray menu item): auto-docking, transparent, always-on-top side shelf (`shelf://`) with native drag-and-drop PNG export, replacing the earlier floating per-shot Pin windows.
- Cross-platform release CI (`.github/workflows/release.yml`), `install.sh` one-liner installer, and Homebrew tap cask publishing.

### Changed
- Startup update check now runs 3 seconds after launch (down from 10 seconds).

## [0.2.3] - 2026-11-21

### Added
- Resize handles (8-point) on selected blur, pixelate, and solid-box redactions.
- 12 color swatches plus a custom system color picker for annotations.
- Light / Dark / System theme switcher in **Settings** (renamed from General).

### Changed
- Streamlined finish buttons: **Copy** copies to clipboard only; **Save** (`Enter`) saves and copies.

### Fixed
- History thumbnails now load via the dedicated `shot://` protocol instead of Tauri's asset protocol, resolving 403 errors under virtualized AppData paths.

## [0.2.2] - 2026-11-21

### Added
- Persistent diagnostic log file (`shutter.log`, accessible via Settings → Show logs).
- Browser mock harnesses in `dev/` (`mock-overlay.html`, `mock-history.html`) for fast UI testing.
- Unified quality gate `npm run check` (TypeScript, JS tests, `cargo clippy -D warnings`, Rust tests), `.githooks/pre-commit`, CI workflow (`.github/workflows/check.yml`), and `scripts/release.ps1`.
- Full-image viewer on click in History, plus select/move/delete support for blur, pixelate, and solid-box redactions.

### Changed
- Settings now always opens on the Shortcuts page.

## [0.2.1] - 2026-11-20

### Fixed
- Eliminated the post-capture visual blink after Copy/Save by hiding the overlay before transferring pixels (~30 ms vs. 160–290 ms previously).

## [0.2.0] - 2026-11-20

### Added
- Signed auto-updates from GitHub Releases (minisign verification) with background update checks on launch and an in-app Update button in Settings.

## [0.1.0] - 2026-11-19

### Added
- Initial tagged release (originally named **Shutter**) built with Tauri 2, TypeScript, and Rust:
  - Fast, DPI-aware multi-monitor region capture overlay (`Ctrl+PrtSc`) with WebView2 shared-memory frame transfer.
  - Destructive local redaction tools (Blur, Pixelate, Solid box) and **Smart Redact** (`S`) for emails, phone numbers, card numbers, IP addresses, and API tokens.
  - Offline on-device OCR (`Windows.Media.Ocr`) with instant text grab (`Ctrl+Shift+PrtSc`), Live Text selection layer, smart action chips, and optional BYO-key AI re-read (Claude / Gemini).
  - Editable annotations: `perfect-freehand` drawing pen with hold-to-snap shapes, cached stroke outlines, thickness/opacity sliders, arrows, boxes, highlighter, text notes, and numbered step badges.
  - Zoom loupe (`Z`), pixel hex color picker (`C`), window snapping, scrolling capture stitching (`L`), and searchable local History with configurable retention and starring.

[Unreleased]: https://github.com/zahidhossin39/SkyShot/compare/v1.0.8...HEAD
[1.0.8]: https://github.com/zahidhossin39/SkyShot/compare/v0.4.1...v1.0.8
[0.4.1]: https://github.com/zahidhossin39/SkyShot/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/zahidhossin39/SkyShot/compare/v0.3.2...v0.4.0
[0.3.2]: https://github.com/zahidhossin39/SkyShot/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/zahidhossin39/SkyShot/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/zahidhossin39/SkyShot/compare/v0.2.3...v0.3.0
[0.2.3]: https://github.com/zahidhossin39/SkyShot/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/zahidhossin39/SkyShot/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/zahidhossin39/SkyShot/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/zahidhossin39/SkyShot/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/zahidhossin39/SkyShot/releases/tag/v0.1.0
