# Screenshot App

Read `SPEC.md` first. It holds every product decision, the roadmap (v1/v2/v3) and spike results.

- Stack: Tauri 2 (TypeScript UI, Rust core), Windows first.
- When a decision changes, update SPEC.md in the same commit.
- Git co-author line: not yet decided. Leave it out until the owner answers (ask before the first push). No git remote yet.

## Status (2026-10-29)

**v1: all features built.** Owner confirmed capture + blur + copy work. Next phase: **v2, then v3** (owner's call). Install/testing/push are deferred.

Deferred v1 items:
- Installer builds (`npm run tauri build`, NSIS only, ~1.7 MB). Not installed yet; waiting on a final app name.
- Untested by hand: Save / Save as, Ctrl+Shift+PrtSc text grab, hotkey rebinding, autostart, multi-monitor, 125/150% DPI.
- Push to GitHub: needs owner OK + co-author answer.
- App name undecided. Shortlist (checked, no app found): Stillpane (top pick), Lensnip, Vitrashot, Glimpshot, Snaptic, Scrimshot. Taken: Veilshot, Hushshot, Cloakshot, Bokashi, Frisket (all screenshot-redaction tools), Snapture, Captureist, Pixsnip.

v2 work happens on branch `experiment/v2` (owner's call). All v2 parts are built: Smart Redact, Live Text + Smart actions, annotations. They're only tested in the browser mock + unit tests; still need a hand test in the real overlay (incl. a link chip actually opening the browser). Then merge to main (owner's call). v3 is underway on the same branch, in this order: pin to screen (done) -> zoom loupe + colour picker + window snap (done) -> searchable history (done) -> AI re-read (IN PROGRESS: ai.rs + settings + overlay button written, WIP commit; NOT yet compiled/tested - run cargo test --lib ai::, then a real call needs the owner's key)  -> scrolling capture.

## How things work (non-obvious)

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
- Window dragging (`startDragging`) swallows mouse-up, so the page never sees `dblclick`. Time the presses yourself.
- Computer-use can't grant the dev build (not an installed app), so the real overlay can't be driven by Claude; ask the owner to hand-test.
- PowerShell `Get-Content -Raw` reads BOM-less UTF-8 as ANSI and mangles `…`/`×`. Use the Edit tool or `[IO.File]` with UTF8Encoding($false).
- The browser pane throttles when hidden: screenshots can show stale frames. Trust `javascript_tool` state over screenshots there.
- Overlay CSS has `[hidden] { display:none !important }` — without it `display:flex` elements ignore `hidden`.
- Toolbar buttons carry `data-name/data-key/data-desc` for the hover tooltip.
- Dev testing: `screenshot-app.exe --capture | --text | --close` drives the running instance. Logs: `[capture] ... visible N ms`, `[ocr] WxH: N ms`.
- UI can be checked in a browser via a temp page that mocks `window.__TAURI_INTERNALS__` and `chrome.webview` (fake frame + invoke stubs), served by the Vite dev server on :1420.
- Dev profile: deps opt-level 3, own crate opt-level 1 (generic image code was ~20x slower unoptimized). Icon changes need `build.rs` touched to re-embed.
- PowerShell 5.1: `Set-Content -Encoding utf8` writes a BOM (breaks serde); commit messages with quotes need `git commit -F file`.
