# Contributing to Sky Shot

Thank you for your interest in contributing to **Sky Shot**! Sky Shot is an open-source, privacy-first screenshot, redaction, and offline OCR desktop app built with Tauri 2 (TypeScript UI + Rust core), licensed under the MIT License (copyright Zaid Hossain / [ZaidHossain.Com](https://zaidhossain.com), built in collaboration with Masum Khan / [TheMasum.Com](https://themasum.com)).

Before starting significant work, please read [`SPEC.md`](SPEC.md) — it is the single source of truth for product decisions, architecture, and intentional non-goals.

---

## Prerequisites

- **Node.js 24+** and `npm`
- **Rust (stable)** with `clippy` (`rustup component add clippy`)
- **Tauri 2 OS prerequisites**:
  - **Windows**: WebView2 (pre-installed on Windows 10/11) and Microsoft C++ Build Tools.
  - **macOS**: Xcode Command Line Tools (`xcode-select --install`).
  - **Linux (Debian / Ubuntu 22.04+)**:
    ```bash
    sudo apt-get update
    sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf
    ```
    Then download the offline `ocrs` ONNX models bundled with Linux builds:
    ```bash
    sh scripts/get-ocr-models.sh
    ```

---

## Getting Started

1. **Fork and clone** the repository:
   ```bash
   git clone https://github.com/zahidhossin39/SkyShot.git
   cd SkyShot
   ```
2. **Install dependencies** (this also configures `.githooks` via `npm run prepare`):
   ```bash
   npm ci
   ```
3. **Run locally**:
   - **Frontend dev server only** (serves on port `1430`):
     ```bash
     npm run dev
     ```
   - **Full desktop app** (Rust + WebView with hot-reloading UI):
     ```bash
     npm run tauri dev
     ```

> **Tip (Windows):** If you run `cargo test` while `npm run tauri dev` is running, set `CARGO_TARGET_DIR=target\test` so both processes don't contend for the Cargo build lock.

---

## Testing UI in Browser Mocks

You can iterate quickly on UI and interaction code without launching the native Tauri shell:

1. Start the Vite dev server (note: Sky Shot uses port **`1430`**, not Tauri's default `1420`):
   ```bash
   npm run dev
   ```
2. Open any mock page in your browser:
   - Overlay: `http://localhost:1430/dev/mock-overlay.html`
   - History: `http://localhost:1430/dev/mock-history.html`
   - Pinning Shelf: `http://localhost:1430/dev/mock-shelf.html`

Each mock stubs Tauri IPC and loads the real page markup and styles, resolving `window.__ready` once the canvas or grid has rendered.

---

## Running Checks

Before opening a pull request, run the full check suite:

```bash
# Fast frontend check (TypeScript typecheck + Node test runner)
npm run check:fast

# Full check (frontend + cargo clippy -D warnings + Rust unit tests)
npm run build && npm run check
```

Notes:
- `tauri::generate_context!` requires the built frontend in `dist/`, so run `npm run build` at least once before `npm run check` on a fresh checkout.
- The `.githooks/pre-commit` hook runs `npm run check:fast` automatically.
- CI (`.github/workflows/check.yml`) runs `npm run build && npm run check` on Windows, macOS, and Ubuntu 22.04.

---

## UI & Material Design 3 Guidelines

Sky Shot uses a minimal **Material Design 3 (M3)** token system defined in [`src/theme.css`](src/theme.css):

- **Use semantic M3 tokens** (`--md-sys-color-*`, tonal surfaces, shape scales, motion easings, state-layer opacities) — avoid hardcoding raw hex values in component styles.
- **Support both Light and Dark themes**: keep the two dark-mode blocks in `src/theme.css` (`@media (prefers-color-scheme: dark)` and `:root[data-theme="dark"]`) identical, and visually verify UI changes in both modes.
- **Keep assets clean**: documentation or GitHub-only images belong in `.github/assets/`, never inside `dist/` or the shipped app bundle.

---

## Updating `SPEC.md`

[`SPEC.md`](SPEC.md) records every product decision, UX rule, and architectural trade-off. If your change alters user-facing behavior, shortcuts, platform behavior, or design decisions, **update `SPEC.md` in the same pull request**.

---

## Pull Request Expectations

- Keep PRs focused on a single bug fix or feature.
- Ensure all text and config files are saved as **UTF-8 without BOM**.
- Verify `npm run check` passes with zero warnings (`clippy -D warnings` is enforced).
- Include before/after screenshots for UI changes (Light & Dark themes).
- Check `SPEC.md` §10 ("Not doing") before proposing features like cloud uploads, share links, or screen recording.
