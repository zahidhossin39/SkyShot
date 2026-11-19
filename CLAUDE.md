# Shutter (screenshot app)

Read `SPEC.md` first. It holds every product decision, the roadmap (v1/v2/v3) and spike results.

- Stack: Tauri 2 (TypeScript UI, Rust core), Windows first.
- When a decision changes, update SPEC.md in the same commit.
- Git co-author line: leave it out (owner, 2026-11-13).
- GitHub: public repo `zahidhossin39/shutter`, MIT. Ask before every push.

## Status (2026-10-29)

**v1: all features built.** Owner confirmed capture + blur + copy work. Next phase: **v2, then v3** (owner's call). Install/testing/push are deferred.

Deferred v1 items:
- Installer builds (`npm run tauri build`, NSIS only, ~1.7 MB). Not installed yet.
- Untested by hand: Save / Save as, Ctrl+Shift+PrtSc text grab, hotkey rebinding, autostart, multi-monitor, 125/150% DPI.
- Push to GitHub: needs owner OK + co-author answer.
- App name: **Shutter** (owner, 2026-11-12). Only the visible name changed; bundle id, exe name and the AI-key vault name stay `screenshot-app` so settings, History and a saved key survive.

v2 work happens on branch `experiment/v2` (owner's call). All v2 parts are built: Smart Redact, Live Text + Smart actions, annotations. They're only tested in the browser mock + unit tests; still need a hand test in the real overlay (incl. a link chip actually opening the browser). Merged to main on 2026-11-13 (owner asked, before the hand test). v3 is underway on the same branch, in this order: pin to screen (done) -> zoom loupe + colour picker + window snap (done) -> searchable history (done) -> AI re-read (done; a real transcription still needs the owner's key) -> scrolling capture (done). v3 is complete; everything still needs the owner's hand test. Since then: the capture screen opens and closes without flicker and stays dim before you select (Lightshot-style), and scrolling capture works through the whole of zaidhossain.com (see SPEC.md). 2026-11-18: fixed bars (chat composer) now appear once, the start toast is gone, steps wait only until the page settles, and a Draw (pen) tool was added. Both still need the owner's hand test in the real overlay.

## How things work (non-obvious)

- Releases + auto-update (v0.2.0+): `tauri-plugin-updater` reads `releases/latest/download/latest.json`. Signing key: `~/.tauri/shutter.key` (no password; pubkey in tauri.conf.json). Losing it = no more updates for installed copies; the owner should back it up. Build with `TAURI_SIGNING_PRIVATE_KEY` set (contents of the key), then attach the `.exe`, `.exe.sig` and a `latest.json` (version, notes, pub_date, platforms.windows-x86_64 {signature = .sig contents, url}) to the GitHub release. Bump the version in tauri.conf.json, Cargo.toml and package.json.
- Update e2e test that worked: build with `-- --config '{"version":"0.1.9"}'` plus a temporary `--update` single-instance hook calling `update::install_update`, install with `/S`, run `--update`; it became 0.2.0 in ~4 s.

- Overlay windows (`overlay-N`, one per monitor) are pre-created hidden. Frame goes Rust -> page via WebView2 shared memory (`capture::post_frame`, `sharedbufferreceived` event), not IPC: IPC cost ~100 ms for 8 MB.
- All overlay geometry is physical pixels; `scale() = W / innerWidth`.
- Redactions are an ops list burned into the canvas, copied to a `base` canvas. Annotations (`notes`, drawn by `annotate.ts`) are redrawn over `base` on every change, so they stay on top. Undo = `history` snapshots of both; it re-blurs only if ops changed.
- Browser mock pane scales oddly; to see the canvas, draw a downscaled copy into a fixed `<img>` top-left and `zoom` on it. Drive input with dispatched MouseEvents in physical px / scale().
- OCR (`ocr.rs`): native pass + parallel 2x pixel-doubled pass; 2x result used if median word height < 16 px. `ocr_samples` / `ocr_big_selection` tests need `OCR_SAMPLES` env (sample PNGs were in the session scratchpad; regenerate if needed).
- `ocr_image` returns `{ text, lines }`: `lines` = word boxes per engine line, in the selection's pixels (2x pass already halved back). `ocr_boxes_match_native_pass` checks that and dumps `<sample>.json` for the JS detector.
- JS unit tests: `npm test` (node's built-in runner, TS run natively; `*.test.ts` is excluded from tsc).
- Browser mock: `ctrl+c` sent by the pane tool and `execCommand('copy')` don't fire a copy event; test the Live Text copy handler by dispatching a `ClipboardEvent('copy', { clipboardData: new DataTransfer() })`.
- Real windows CAN be tested from PowerShell via Win32 (Add-Type): FindWindow by title (pass `[NullString]::Value`, not `$null`, for the class), call SetProcessDpiAwarenessContext(-4) first or rects come back scaled, then SendKeys / mouse_event for input. A temporary CLI hook in the single-instance handler can trigger a feature (remove before commit).
- To test a page inside a real window: temporary hook calls `window.eval(js)`, and the JS reports back through a temporary `debug_log` command that prints to dev.log. Remove both before commit.
- Create windows only from async commands or main-thread event handlers. A window built in a sync command (`open_history_window` was) shows up but never closes (WebView2 deadlock).
- Real-app test scripts live in the session scratchpad and are lost with it (the 2026-11 set was gone by 2026-11-18). What worked: `open-test-window.ps1 <url> <title>` opens an app-mode Chrome window with a throwaway `--user-data-dir` (its own processes: close it by killing chrome.exe whose command line has that profile, never the owner's Chrome) and prints its physical client rect; then `SCROLL_TEST="x,y,w,h,out.png" cargo test real_scroll_capture -- --nocapture` runs the real stitcher on it (no History entry). Test pages: a chat with a fixed composer + spinner + sticky header, and 300 near-identical monospace lines.
- Check a stitch by OCR, not by eye: put the PNG alone in a folder, `OCR_SAMPLES=<folder> cargo test ocr_samples -- --nocapture`, count each "Message N"/"Line N". Windows OCR fails on images over 10,000 px tall: split first. A few misses are OCR, so look at those rows before calling it a stitch bug.
- zaidhossain.com is the hard scroll-capture case: fade-ins, an accordion that expands on scroll, a sticky column, a pinned section. Notepad's near-identical lines are the other hard case (repeating rows). Check both after any change to `scroll.rs`.
- Run `cargo test` with `CARGO_TARGET_DIR=target\test` while `tauri dev` runs, or the two fight over the build lock. The dev watcher rebuilds between quick edits; a linker error (LNK2019) after that means a corrupt incremental build. Fix: stop dev, `cargo clean -p screenshot-app`.
- Window dragging (`startDragging`) swallows mouse-up, so the page never sees `dblclick`. Time the presses yourself.
- Computer-use can't grant the dev build (not an installed app), so the real overlay can't be driven by Claude; ask the owner to hand-test.
- PowerShell `Get-Content -Raw` reads BOM-less UTF-8 as ANSI and mangles `…`/`×`. Use the Edit tool or `[IO.File]` with UTF8Encoding($false).
- The browser pane throttles when hidden: screenshots can show stale frames. Trust `javascript_tool` state over screenshots there.
- Overlay CSS has `[hidden] { display:none !important }` — without it `display:flex` elements ignore `hidden`.
- Toolbar buttons carry `data-name/data-key/data-desc` for the hover tooltip.
- The pen uses pointer events and `preventDefault`s pointerdown, so its presses never reach the overlay's mousedown handler. Synthetic PointerEvents can't be captured: stub `Element.prototype.setPointerCapture` when driving the pen from JS.
- Browser mock of the overlay: `mock-overlay.html` (git-excluded, project root) fakes Tauri + the shared-buffer frame; open `http://localhost:1430/mock-overlay.html` with the dev server up. If missing, rebuild it: `__TAURI_INTERNALS__.invoke` answers `overlay_pending` / `overlay_frame` (dispatch a `sharedbufferreceived` event whose `getBuffer()` returns the RGBA buffer) / `overlay_shown`, `chrome.webview` is an EventTarget with `releaseBuffer`, and the body is copied from overlay.html. The pane must be visible (innerWidth 0 when hidden).
- Dev server port is 1430, not Tauri's default 1420: the owner's Silent Voice project also uses 1420, and both bound it (IPv4 vs IPv6) so pages loaded from the wrong app.
- Settings and History share `src/theme.css` tokens (light + dark).
- Dev testing: `screenshot-app.exe --capture | --text | --close` drives the running instance. Logs: `[capture] ... visible N ms`, `[ocr] WxH: N ms`.
- UI can be checked in a browser via a temp page that mocks `window.__TAURI_INTERNALS__` and `chrome.webview` (fake frame + invoke stubs), served by the Vite dev server on :1430 (see `mock-overlay.html` below).
- Dev profile: deps opt-level 3, own crate opt-level 1 (generic image code was ~20x slower unoptimized). Icon changes need `build.rs` touched to re-embed.
- PowerShell 5.1: `Set-Content -Encoding utf8` writes a BOM (breaks serde); commit messages with quotes need `git commit -F file`.
