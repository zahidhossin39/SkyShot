# Shutter

A fast screenshot tool for Windows, built with Tauri 2 (Rust core, TypeScript UI).

## Features

- **Capture:** Ctrl+PrtSc freezes the screen, dimmed until you drag a selection. Hover a window to snap to it.
- **Blur / redact:** drag over anything private and it's blurred for good.
- **Text grab (OCR):** Ctrl+Shift+PrtSc copies the text in a selection. Uses the Windows OCR engine, offline.
- **Copy, save, save as**, plus a tray icon, settings, hotkey rebinding and autostart.

On the `experiment/v2` branch (not yet hand-tested):

- Smart Redact (finds emails, keys, card numbers), Live Text with smart actions, annotations
- Pin to screen, zoom loupe + colour picker, searchable History, AI re-read, scrolling capture

## Run from source

Needs [Node.js](https://nodejs.org), [Rust](https://rustup.rs) and the WebView2 runtime (built into Windows 11).

```powershell
npm install
npm run tauri dev
```

Build an installer: `npm run tauri build`.

Tests: `npm test` (JS) and `cargo test` in `src-tauri`.

## License

[MIT](LICENSE)
