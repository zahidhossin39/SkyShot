# Shutter (screenshot app)

Read `SPEC.md` first. It holds every product decision, the roadmap (v1/v2/v3) and spike results.

- Stack: Tauri 2 (TypeScript UI, Rust core), Windows first.
- When a decision changes, update SPEC.md in the same commit.
- Git co-author line: leave it out (owner, 2026-11-13).
- GitHub: public repo `zahidhossin39/shutter`, MIT. Ask before every push.

Status, what's released and the hand-test list: SPEC.md §11.

## Testing in the browser pane

Before each run:
1. Start the dev server (`npm run dev`, or `npm run tauri dev`) and stop it when done; a leftover one holds port 1430.
2. Open `http://localhost:1430/dev/mock-overlay.html` or `/dev/mock-history.html`. They fake Tauri and load the real page's markup, so they never go stale.
3. Make sure the pane is showing (`tabs_select`). A hidden pane has `innerWidth` 0, freezes animation frames and times scripts out after 45 s.
4. Start each script with `await window.__ready`, which resolves once the frame or the grid is drawn.
5. Drive input with dispatched events in physical px divided by `scale()`. Trust `javascript_tool` values over screenshots; the pane scales oddly and screenshots can be stale.

## How things work (non-obvious)

- Checks: `npm run check` (typecheck, JS tests, clippy -D warnings, Rust tests). The pre-commit hook runs the fast half; CI (`.github/workflows/check.yml`) runs all of it on push.
- Releases: `scripts/release.ps1 -Version x.y.z -Notes "..." -NotesFile notes.md` (try `-DryRun` first). It bumps every version file, checks, builds signed, writes `latest.json`, pushes and publishes. Installed copies update from that `latest.json`. The signing key `~/.tauri/shutter.key` (no password) must never be lost: without it, installed copies can't update.
- Update e2e test that worked: build with `-- --config '{"version":"0.1.9"}'` plus a temporary `--update` single-instance hook calling `update::install_update`, install with `/S`, run `--update`; it became 0.2.0 in ~4 s.

- Overlay windows (`overlay-N`, one per monitor) are pre-created hidden. Frame goes Rust -> page via WebView2 shared memory (`capture::post_frame`, `sharedbufferreceived` event), not IPC: IPC cost ~100 ms for 8 MB.
- All overlay geometry is physical pixels; `scale() = W / innerWidth`.
- Redactions are an ops list burned into the canvas, copied to a `base` canvas. Annotations (`notes`, drawn by `annotate.ts`) are redrawn over `base` on every change, so they stay on top. Undo = `history` snapshots of both; it re-blurs only if ops changed.
- OCR (`ocr.rs`): native pass + parallel 2x pixel-doubled pass; 2x result used if median word height < 16 px. `ocr_samples` / `ocr_big_selection` tests need `OCR_SAMPLES` env (sample PNGs were in the session scratchpad; regenerate if needed).
- `ocr_image` returns `{ text, lines }`: `lines` = word boxes per engine line, in the selection's pixels (2x pass already halved back). `ocr_boxes_match_native_pass` checks that and dumps `<sample>.json` for the JS detector.
- JS unit tests: `npm test` (node's built-in runner, TS run natively; `*.test.ts` is excluded from tsc).
- Browser mock: `ctrl+c` sent by the pane tool and `execCommand('copy')` don't fire a copy event; test the Live Text copy handler by dispatching a `ClipboardEvent('copy', { clipboardData: new DataTransfer() })`.
- Real windows CAN be tested from PowerShell via Win32 (Add-Type): FindWindow by title (pass `[NullString]::Value`, not `$null`, for the class), call SetProcessDpiAwarenessContext(-4) first or rects come back scaled, then SendKeys / mouse_event for input. A temporary CLI hook in the single-instance handler can trigger a feature (remove before commit).
- To test a page inside a real window: temporary hook calls `window.eval(js)`, and the JS reports back through a temporary `debug_log` command that prints to dev.log. Remove both before commit.
- Create windows only from async commands or main-thread event handlers. A window built in a sync command (`open_history_window` was) shows up but never closes (WebView2 deadlock).
- Real-app test scripts (test window, blink probe, scroll capture on a real page): `scripts/dev/README.md`.
- Check a stitch by OCR, not by eye: put the PNG alone in a folder, `OCR_SAMPLES=<folder> cargo test ocr_samples -- --nocapture`, count each "Message N"/"Line N". Windows OCR fails on images over 10,000 px tall: split first. A few misses are OCR, so look at those rows before calling it a stitch bug.
- zaidhossain.com is the hard scroll-capture case: fade-ins, an accordion that expands on scroll, a sticky column, a pinned section. Notepad's near-identical lines are the other hard case (repeating rows). Check both after any change to `scroll.rs`.
- Run `cargo test` with `CARGO_TARGET_DIR=target\test` while `tauri dev` runs, or the two fight over the build lock. The dev watcher rebuilds between quick edits; a linker error (LNK2019) after that means a corrupt incremental build. Fix: stop dev, `cargo clean -p screenshot-app`.
- Window dragging (`startDragging`) swallows mouse-up, so the page never sees `dblclick`. Time the presses yourself.
- Never start or restart the owner's installed Shutter from Claude's tools (nor let the updater restart it from there). Claude desktop is a packaged (MSIX) app, so its child processes get AppData virtualized: new files land in `AppData\Local\Packages\Claude_*\LocalCache\Roaming\…` and settings diverge. That made every History image 403 in 0.2.2 (fixed by the `shot://` protocol). Stop it if needed, then ask the owner to start it from the Start menu.
- Computer-use can't grant the dev build (not an installed app), so the real overlay can't be driven by Claude; ask the owner to hand-test.
- PowerShell `Get-Content -Raw` reads BOM-less UTF-8 as ANSI and mangles `…`/`×`. Use the Edit tool or `[IO.File]` with UTF8Encoding($false).
- Overlay CSS has `[hidden] { display:none !important }` — without it `display:flex` elements ignore `hidden`.
- Toolbar buttons carry `data-name/data-key/data-desc` for the hover tooltip.
- The pen uses pointer events and `preventDefault`s pointerdown, so its presses never reach the overlay's mousedown handler. Synthetic PointerEvents can't be captured: stub `Element.prototype.setPointerCapture` when driving the pen from JS.
- Dev server port is 1430, not Tauri's default 1420: the owner's Silent Voice project also uses 1420, and both bound it (IPv4 vs IPv6) so pages loaded from the wrong app.
- Settings and History share `src/theme.css` tokens (light + dark).
- Dev testing: `screenshot-app.exe --capture | --text | --close` drives the running instance (stop the installed copy first: they share one instance). Logs: `crate::log!` prints and appends to `%APPDATA%\com.zaid-hossain.screenshot-app\logs\shutter.log` (Settings → General → Show logs), so the owner's machine has timings and errors too.
- Dev profile: deps opt-level 3, own crate opt-level 1 (generic image code was ~20x slower unoptimized). Icon changes need `build.rs` touched to re-embed.
- PowerShell 5.1: `Set-Content -Encoding utf8` writes a BOM (breaks serde); commit messages with quotes need `git commit -F file`.
