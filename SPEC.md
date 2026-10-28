# Screenshot App — Spec

Single source of truth for this project. Decisions here came from a grilling session on 2026-10-28.
If a decision changes, update this file in the same commit.

## 1. Summary

| | |
|---|---|
| **Outcome** | A Windows screenshot tool better than Lightshot, with blur/redact and text extraction (OCR) built in. |
| **User** | The owner (solo dev, learning as they build), for daily personal use. Maybe public later. |
| **Why now** | Windows ships a free offline OCR engine; Tauri 2 gives a light app with a cross-platform path. |
| **Success** | After 2 weeks of daily use, Lightshot and Snipping Tool are uninstalled and never missed. Hotkey to overlay visible in **< 200 ms**. |
| **Constraint** | Solo learner. v1 must be usable daily within ~1–2 weeks. |
| **Out of scope** | Upload / share links, screen recording / GIF, beautify/mockup, "ask AI anything". |

## 2. Platform & stack

- **Tauri 2**: UI in HTML + TypeScript, a small Rust core for capture, hotkeys, tray and OCR.
  - Why: ~10 MB, fast startup (needed for a hotkey app), and it targets Win/Mac/Linux, plus Android/iOS later.
- **Windows first.** Cross-platform (Mac, Linux, Android, iOS) comes later.
  - Mobile caveat: phones already take screenshots, and iOS forbids capturing other apps. On mobile this app is an **editor** (blur + OCR on images you open), not a capture tool.
  - Design OS-specific pieces (capture, OCR, hotkeys) behind a small interface so each OS can get its own implementation.
- Package manager: `npm`. Shell: PowerShell (Windows).

## 3. Core UX

### Capture flow (Lightshot-style, edit in place)
1. Press hotkey → screen freezes → dim overlay covers **all monitors**.
2. Drag a region.
3. A toolbar appears **right next to the selection**. There is no separate editor window.
4. Edit (blur, OCR, etc.).
5. Finish → **ask every time** via toolbar buttons: **Copy / Save / Copy & save**.
   - Save default location: `Pictures\Screenshots\<date-time>.png`. Ctrl+S = save as. Enter = Copy & save.
   - Esc cancels.
   - Every toolbar button has a hover tooltip: name, shortcut key, and one line on what it does. A redaction tool shows a hint pill at the top while it's active.
   - Text reading (OCR) runs only when asked for: the **Text** button or T.

### Displays
- Multi-monitor and mixed DPI scaling (100/125/150 %) must work from v1.
  - Why: this is the #1 cause of blurry or offset captures in screenshot tools, and it's costly to retrofit.

### Hotkeys
- All defaults are **2-key** and **fully rebindable in settings (v1)**.
- Windows defaults:
  - `Ctrl+PrtSc`: region capture
  - `Ctrl+Shift+PrtSc`: instant text grab (drag region → text to clipboard, no image). This is the one 3-key default, because every free 2-key PrtSc combo is taken: Lightshot owns PrtSc and Shift+PrtSc, Windows owns Alt and Win+PrtSc, and capture uses Ctrl+PrtSc. "Add Shift for text" is easy to remember.
- Leave Windows' own `PrtSc`, `Win+PrtSc`, `Alt+PrtSc` alone.
- Settings must **warn when a combo is already taken**. Windows `RegisterHotKey` fails if another app owns it, so it can be detected.
- Each OS gets its own defaults. Mac later: `Cmd+Shift+3/4/5` are reserved by the OS, so use something like `Option+S` / `Option+T`.

## 4. Feature: Blur / redact

- Drag a box over any area. Everything under it gets hidden.
- **Three modes, user picks per use:** Blur, Pixelate, Solid box.
  - Show a hint that **blurred or pixelated text can sometimes be reversed by deblur tools. Use Solid box for passwords and keys.**
- Redaction is burned into the exported image (pixels destroyed), not a layer on top.

### Smart Redact (v2)
- OCR finds sensitive text: emails, phone numbers, card numbers, API keys/tokens, IP addresses.
- Draws **dashed suggestion boxes** over each one. Nothing is blurred until the user confirms.
- One click "Redact all", or untick individual ones.
- **Click any word** to redact just that word.
- Not fully automatic. Why: auto-hiding can remove things you meant to show, and you may not notice.
- **Built (experiment/v2 branch):** toolbar shield button or `S`. Red dashed boxes = will be hidden; click a box to keep it, click any other word to add it; `Enter` redact, `Esc` back. Uses Black box unless Blur/Pixelate is the active tool. The whole batch is one undo step.
- Detection is regex on each OCR line (`src/detect.ts`, tests in `detect.test.ts`, run `npm test`): email, IPv4 (port included), card (Luhn check), phone (9–15 digits, 8 if it starts with `+`), key/token (any 20+ char letters+digits token, plus an `sk-`/`ghp_`-style prefix word because OCR splits it). Known false positive: a plain 10-digit number reads as a phone. Accepted since every box is reviewed.

## 5. Feature: Text extraction (OCR)

### Engines
- **Default: offline, on-device**: Windows built-in OCR (`Windows.Media.Ocr`). Free, instant, private.
- **AI fallback (v3)**: "Re-read with AI" button for blurry text, handwriting or odd fonts.
  - **Bring your own API key** (Claude or Gemini) pasted in settings. No server.
  - Only the image the user clicks AI on gets sent.
  - **Scope: better OCR only.** No translate or Q&A for now (easy to add later).

### OCR UX (all four wanted; staged across versions)
| Mode | What it does | Version |
|---|---|---|
| Copy-all | Click OCR → popup with all text, editable → copy | v1 |
| Instant text hotkey | `Ctrl+Shift+PrtSc` → drag → text straight to clipboard, no image | v1 |
| Live Text | Text in the captured image becomes selectable like a web page; highlight words and copy | v2 |
| Smart actions | Detect links / emails / phones / code → one click opens a URL, copies code as a block, etc. | v2 |

**Built (experiment/v2):** Live Text and Smart actions both hang off the **Text** button (`T`). One OCR pass fills the panel, the live layer and the chips.
- Live Text: invisible OCR words laid over the selection (like a PDF text layer). Drag to highlight, `Ctrl+C` copies the selected words (whole words, one line per OCR line) instead of the image. `Esc` closes the panel and the layer. Picking a redaction tool also closes it, since the layer would catch the drag.
- Smart actions: up to 3 chips in the panel: "Open <link>", "Copy <email>", "Copy <phone>". Links open in the default browser and close the overlay. Rust only opens `http(s)://` links with no spaces or quotes (`is_web_link`), because the text comes from a screenshot.
- Not done: "copy code as a block". The panel's Copy all already keeps line breaks.

## 6. Other features

| Feature | Notes | Version |
|---|---|---|
| Annotations | Arrows, boxes, text, highlighter, numbered step badges (1, 2, 3). Editable after placing. | v2 |
| Searchable history | Every capture saved with OCR text indexed; search "invoice" to find it. | v3 |
| Pin to screen | Float a screenshot always-on-top as a reference. | v3 |
| Smart snap + magnifier | While dragging: snap to windows/UI elements, zoom loupe, pixel color with copyable hex. | v3 |
| AI re-read | See §5. | v3 |
| Scrolling capture | Auto-scroll and stitch long pages/chats. Hard. | v3 |

### Annotations (built on experiment/v2)
- Tools: Arrow `A`, Box outline `O`, Highlighter `H` (rectangle, multiply blend so text stays crisp), Text note `W`, Steps `N` (numbered badges that renumber when one is deleted). Colour `C` cycles red / yellow / green / blue / black.
- Editable after placing: with Select (`V`), click a mark to pick it (dashed outline), drag to move, `Del` deletes it, `C` recolours it, double-click a text note to edit it. Text: `Enter` done, `Shift+Enter` new line, `Esc` discard, clicking away also finishes it.
- Annotations always sit above redactions: a blur drawn over an arrow never smears the arrow. Why: the redacted image is kept as a separate base canvas, and notes are redrawn on top. That also makes moving a note cheap, with no re-blur.
- Undo is a snapshot history of redactions and notes, so `Ctrl+Z` also reverts moves, edits, recolours and deletes.
- Not done (skipped on purpose): freehand pen, resizing a placed mark (redraw it instead), font size picker. The toolbar is ~800 CSS px wide now, which fits 1366×768 at 125%.

### Pin to screen (built on experiment/v2)
- Toolbar pin button or `F`: the overlay closes and the selection floats in an always-on-top window, placed exactly where it was captured (annotations and redactions included).
- Drag to move. Scroll to zoom (10% steps, 20–500%), and `0` resets to 100%. `Ctrl+C` copies it. `Esc`, double-click or the hover × closes it. Any number of pins can be open.
- Rust keeps each pin's pixels (`pin.rs`, dropped when its window closes) and gives the page a PNG. Pin windows get their own capability (`capabilities/pin.json`: drag, resize, show, focus, close) instead of widening the overlay's.
- Tested on the real window via Win32: position, 400×300 client size, zoom, reset, drag, Esc, double-click, Ctrl+C.

### Zoom loupe, colour picker, window snap (built on experiment/v2)
- Loupe: while choosing an area (before selecting, while dragging a new one, or while resizing an edge), a 120 px magnifier follows the cursor. It shows 15×15 pixels at 8×, the centre pixel outlined, its hex and its x,y. It flips sides near screen edges.
- `C` (while the loupe shows) copies the hex, e.g. `#1E40AF`. The colour comes from the original screenshot, not from our marks.
- Window snap: before selecting, the window under the cursor is outlined; a click (no drag) selects exactly that window. Rust lists windows after the frames are sent (xcap, z-order, own windows and windows under 40 px dropped, clipped per monitor). That took ~3 ms in testing, off the hotkey path (hotkey to visible unchanged at ~170 ms). Rects matched DPI-aware Win32 rects exactly.
- Not done: snapping to UI elements inside a window (buttons, panels). That needs UI Automation, so it's a later maybe.

### History rules (v3)
- **Retention is user-configurable in settings**: number of days (e.g. 1–365) or "forever". Default 30.
  - Changing the value applies to existing shots too (shorten → older ones get deleted).
- **Star** a shot to keep it forever, regardless of retention.
- **Stores the redacted version only.** Text under redacted areas is **never indexed**.
  - Why: a password you hid should never be findable by search or sit on disk.

### Searchable history (built on experiment/v2)
- **What's saved:** every finished capture (Copy, Save, Copy & save, Save as when not cancelled, Pin), in the background after the overlay closes. It is saved exactly as exported, so redactions and annotations are burned in and OCR runs on that image. Instant text grab is not saved, since it has no image.
- **Storage:** app data dir `history/`, holding `<id>.png`, `<id>.thumb.png` (fits 480×320) and `index.json` (id, time, size, OCR text, starred). One JSON index, no database. The index is written to a temp file and renamed, so a crash can't wipe it. A 600×400 shot took ~60 ms to save, OCR included.
- **History window** (tray → History…, or Settings → Open history): search box (every word must appear, any order and case, matched words shown in a snippet), a Starred filter, and a newest-first grid. Click a thumbnail or Copy to copy it. Show in folder. Star to keep forever. Delete takes two clicks ("Sure?"). `Ctrl+F` or `/` focuses search.
- **Retention:** Settings → History, 1 / 7 / 30 / 90 / 365 days or Forever (default 30). It applies on change (asks first when shortening), after each save, and at startup.
- **Safety:** ids from the page are checked before they become file names, so they can't escape the folder. Thumbnails load through Tauri's asset protocol, scoped to `$APPDATA/history/**` only.
- Tested in the real app with a temporary hook: save + OCR + files + index, thumbnail loads in the real window, search hit/miss with snippet, star saved to disk, copy, two-click delete removes the files.

## 7. Roadmap

### v1: core loop (target ~1–2 weeks)
- Tray app, starts with Windows (optional).
- Global hotkeys (2-key defaults) + settings screen to rebind them, with conflict warning.
- In-place region capture across all monitors, DPI-correct.
- Blur / Pixelate / Solid box.
- Offline OCR: copy-all popup + instant text hotkey.
- Finish prompt: Copy / Save / Copy & save.

### v2: differentiators
- Live Text, Smart actions, Smart Redact, annotation tools.

### v3: power features
- Searchable history, pin to screen, smart snap + magnifier/color picker, AI re-read (BYO key), scrolling capture.

### Later / maybe
- Mac, Linux, then mobile (editor-only).

## 8. Build order for v1 (de-risk first)

1. **Spike: Windows OCR quality.** Run `Windows.Media.Ocr` on ~10 real screenshots. If quality is bad, rethink the engine (e.g. Tesseract, PaddleOCR) before building UI.
2. **Spike: overlay speed.** Tauri transparent overlay across all monitors, measure hotkey→visible. Must be < 200 ms. If not, consider a pre-created hidden window kept warm.
3. Scaffold the Tauri 2 app (tray, single instance).
4. Global hotkeys + settings persistence.
5. Region selection overlay, multi-monitor + DPI.
6. Toolbar + blur/pixelate/box.
7. OCR copy-all + instant text grab.
8. Copy / Save / Both.
9. Use it daily; fix friction.

## 9. Assumptions to validate

| Assumption | Cheapest test | Kills / changes the plan if… |
|---|---|---|
| Windows OCR is good enough on real screenshots | Spike #1 above | Poor accuracy → swap engine |
| Tauri overlay can appear in < 200 ms on multiple monitors | Spike #2 above | Too slow → keep a warm hidden window, or reconsider stack |
| Blur/pixelate is acceptable for most redaction | Use it daily | Solid box already covers secrets |

### Spike results (2026-10-28)

- **OCR:** Windows OCR in Rust gets ~100% on prose, UI text, URLs, and keys. Small text needs upscaling. Rule: a 2× pass runs in parallel with the native pass, and its result is used only if the median word height is under 16 px. Upscaling everything hurt normal text, so the threshold stays.
  - The 2× upscale is plain pixel doubling. It reads as well as a smooth resize and costs 12 ms instead of 600+ ms on a 1200×800 area.
  - Timing (dev build): 7–34 ms for typical selections. The worst case is a 1200×800 area of small text at ~430 ms, which used to take ~4 s.
  - The dev profile builds our own crate with `opt-level = 1`, because generic image code compiled unoptimized was ~20× slower.
- **Overlay speed (debug build, 1 monitor, warm hidden window):** hotkey→visible is **95–111 ms** when the machine is idle and ~170–200 ms under load. Sending the 8 MB frame through Tauri IPC took 88–124 ms. The fix is to post it as WebView2 shared memory (`PostSharedBufferToScript`), which takes 14–35 ms. Fast PNG was tried and was slower: encoding plus decoding cost more than the transfer it saved.
- **Hotkey conflict:** running Lightshot owns `Shift+PrtSc`. That left the text grab dead, so the default moved to `Ctrl+Shift+PrtSc`.
- v1 selection is per monitor; it can't span monitors.
- CLI flags `--capture` / `--text` / `--close` trigger or end a capture through the running instance, which is handy for testing and shortcuts.

## 10. Not doing (and why)

- **Upload / share links.** Lightshot's public, guessable URLs have leaked people's data. Local only: clipboard, save, drag-drop.
- **Screen recording / GIF.** It's basically a second app.
- **Beautify / mockup.** Nice to have, not the owner's pain.
- **Ask-AI-anything / translate.** Not wanted now; trivial to add once the AI hookup exists.
- **Fully automatic redaction.** Too easy to silently hide the wrong thing.

## 11. Open items

- Git co-author line (`Co-Authored-By: Claude ...`): ask the owner once before the first push to GitHub, then record the answer in `CLAUDE.md`. Until answered, leave it out.
