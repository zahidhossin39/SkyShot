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

v2 work happens on branch `experiment/v2` (owner's call). Order: Smart Redact (done, needs a hand test in the real overlay) -> Live Text + Smart actions -> annotations.

## How things work (non-obvious)

- Overlay windows (`overlay-N`, one per monitor) are pre-created hidden. Frame goes Rust -> page via WebView2 shared memory (`capture::post_frame`, `sharedbufferreceived` event), not IPC: IPC cost ~100 ms for 8 MB.
- All overlay geometry is physical pixels; `scale() = W / innerWidth`.
- Redactions are an ops list burned into the canvas; undo replays from the original ImageData.
- OCR (`ocr.rs`): native pass + parallel 2x pixel-doubled pass; 2x result used if median word height < 16 px. `ocr_samples` / `ocr_big_selection` tests need `OCR_SAMPLES` env (sample PNGs were in the session scratchpad; regenerate if needed).
- `ocr_image` returns `{ text, lines }`: `lines` = word boxes per engine line, in the selection's pixels (2x pass already halved back). `ocr_boxes_match_native_pass` checks that and dumps `<sample>.json` for the JS detector.
- JS unit tests: `npm test` (node's built-in runner, TS run natively; `*.test.ts` is excluded from tsc).
- The browser pane throttles when hidden: screenshots can show stale frames. Trust `javascript_tool` state over screenshots there.
- Overlay CSS has `[hidden] { display:none !important }` — without it `display:flex` elements ignore `hidden`.
- Toolbar buttons carry `data-name/data-key/data-desc` for the hover tooltip.
- Dev testing: `screenshot-app.exe --capture | --text | --close` drives the running instance. Logs: `[capture] ... visible N ms`, `[ocr] WxH: N ms`.
- UI can be checked in a browser via a temp page that mocks `window.__TAURI_INTERNALS__` and `chrome.webview` (fake frame + invoke stubs), served by the Vite dev server on :1420.
- Dev profile: deps opt-level 3, own crate opt-level 1 (generic image code was ~20x slower unoptimized). Icon changes need `build.rs` touched to re-embed.
- PowerShell 5.1: `Set-Content -Encoding utf8` writes a BOM (breaks serde); commit messages with quotes need `git commit -F file`.
