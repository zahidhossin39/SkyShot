# Shutter — Spec

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
1. Press hotkey → screen freezes → dim overlay covers **all monitors**. Like Lightshot, the whole screen stays dim until you drag; only the selection is clear. Hovering a window only outlines it.
   - Smooth open/close: Windows' own show/hide animation is off on the overlay (DWM `TRANSITIONS_FORCEDISABLED`). The window is shown only after the frozen frame is painted, then the dim layer fades in (140 ms). On close or finish, the UI fades out (110 ms) and the window hides before it resets, so no black flash. Measured: no black frame either way.
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
  - **Built (experiment/v2):** "Re-read with AI" button in the Text panel replaces the panel text with the AI's transcription. Settings → AI re-read: pick Claude or Gemini, paste key; the key is stored in Windows Credential Manager (never in settings.json, never sent to the page, shown as "…ab12").
  - Claude: Messages API, `claude-opus-5-5`, effort medium, `fallbacks: "default"` (safety decline re-runs on Anthropic's fallback model). Gemini: `v1beta` generateContent, `gemini-2.5-flash` (key in `x-goog-api-key` header). HTTPS via Windows' own `Windows.Web.Http`, no new crate.
  - The image sent is the cropped, already-redacted selection, so hidden text never leaves the PC.
  - Tested: request/response parsing unit tests; a real round trip to both APIs with a fake key (gets their "bad key" errors back as plain messages, set `AI_NET_TEST=1`); Credential Manager save/replace/remove. Not tested: a real transcription (needs the owner's key).

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
| Annotations | Pen (freehand, snaps to shapes), arrows, boxes, text, highlighter, numbered step badges (1, 2, 3). Editable after placing. | v2 |
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
- Not done (skipped on purpose): resizing a placed mark (redraw it instead), font size picker. The toolbar is ~830 CSS px wide now, which fits 1366×768 at 125%.

### Pen / Draw (added 2026-11-18, owner asked for "one of the best drawing options, but minimal")
- `D` or the pencil button. Built from a research pass (tldraw, Excalidraw, Apple Markup, Procreate QuickShape, FigJam, CleanShot X):
  - Strokes use `perfect-freehand` (MIT, ~3 kB, the algorithm behind tldraw and Excalidraw): a filled outline, smoothed and streamlined. Pen tablets use real pressure; a mouse gets pressure from speed (fast = thinner), which is what makes it feel like ink.
  - Pointer events with coalesced samples, so fast strokes stay curves; one redraw per frame. `touch-action: none` so a pen stroke isn't taken as a Windows pan gesture.
  - **Hold to snap:** hold still ~0.5 s before letting go and a rough line, circle/ellipse or box becomes a clean one; keep dragging to resize it (a line's end follows, a box/ellipse scales about its centre). Lines within 5° of level/upright are straightened. Recognition (`ink.ts`, unit-tested): line if the ends are ≥92% as far apart as the path is long (path measured on thinned points, since hand jitter added 20–30% to a straight stroke's length); closed shapes are a box if the path passes near all 4 bounding-box corners, an ellipse if not; scribbles and open curves stay ink.
  - `Shift` draws a straight line at 45° steps.
  - **Thickness (1–30 px) and Opacity (10–100%) sliders** in a small panel attached to the toolbar by the Draw button (shown for the pen or a picked stroke; placed on the side away from the selection). `[` `]` step the thickness. A slider drag on a picked stroke is one undo step. The toolbar itself didn't grow (it's ~950 CSS px).
  - **Cursor = Windows' crosshair + a size ring drawn in the canvas.** A custom dot cursor looked off from where ink landed at 150% scaling (owner report); measured on the real overlay, ink lands within 1 px of the pointer, so the fix was the cursor, not the drawing. The ring uses the same pixels as the ink.
  - Strokes are ordinary marks: pick with Select, move, recolour, resize, delete, undo.
- Skipped on purpose (no clutter): a separate eraser (Select + `Del` and `Ctrl+Z` cover it), a freehand highlighter (the box highlighter fits screenshots of text better), a colour picker beyond the 5 presets, low-latency `desynchronized` canvas (the screenshot canvas has toolbars over it, which defeats it).

### Pin to screen (built on experiment/v2)
- Toolbar pin button or `F`: the overlay closes and the selection floats in an always-on-top window, placed exactly where it was captured (annotations and redactions included).
- Drag to move. Scroll to zoom (10% steps, 20–500%), and `0` resets to 100%. `Ctrl+C` copies it. `Esc`, double-click or the hover × closes it. Any number of pins can be open.
- Rust keeps each pin's pixels (`pin.rs`, dropped when its window closes) and gives the page a PNG. Pin windows get their own capability (`capabilities/pin.json`: drag, resize, show, focus, close) instead of widening the overlay's.
- Tested on the real window via Win32: position, 400×300 client size, zoom, reset, drag, Esc, double-click, Ctrl+C.

### Zoom loupe, colour picker, window snap (built on experiment/v2)
- Loupe: OFF by default (owner found the always-on magnifier annoying); press `Z` to toggle it per capture. When on, while choosing an area (before selecting, while dragging a new one, or while resizing an edge), a 120 px magnifier follows the cursor. It shows 15×15 pixels at 8×, the centre pixel outlined, its hex and its x,y. It flips sides near screen edges.
- `C` (while the loupe shows) copies the hex, e.g. `#1E40AF`. The colour comes from the original screenshot, not from our marks.
- Window snap: before selecting, the window under the cursor is outlined (screen stays dim); a click (no drag) selects exactly that window. Rust lists windows after the frames are sent (xcap, z-order, own windows and windows under 40 px dropped, clipped per monitor). That took ~3 ms in testing, off the hotkey path (hotkey to visible unchanged at ~170 ms). Rects matched DPI-aware Win32 rects exactly.
- Not done: snapping to UI elements inside a window (buttons, panels). That needs UI Automation, so it's a later maybe.

### Scrolling capture (built on experiment/v2)
- Toolbar button or `L`: the overlay closes, the selected area is scrolled with the mouse wheel and grabbed after each step, and frames are stitched by matching pixel rows (top/bottom 10% ignored so sticky headers don't break it). Stops when the page hasn't moved for 4 steps in a row, or after 150 steps / 20,000 px. Fewer than 4 cut off a pinned section, where the page holds still while you scroll. Result is copied, saved to History, and a notification shows its size. Cursor is put back afterwards.
- **Fixed bars are added once.** Rows at the bottom that stay put while the page scrolls (a chat app's message box, a sticky footer, a cookie bar) used to be stitched in on every step, so the Claude app's composer repeated down the image. Now each step stitches from just above them and they're added once at the end. Found per strip (a spinner in the message box only breaks its own strip) and only when a non-blank row anchors it (plain margins prove nothing). Counting too many rows is harmless: the page rows held back come in from the next frame. Same idea as ShareX's "ignore bottom edge" and Stackshot's insert-before-footer.
- **No "hands off" notification at the start.** It slid into the captured corner and got stitched in (grey boxes down the image), and its slide-in looked like movement and appended the composer again. The hint lives in the button's tooltip now; Windows offers no supported way to hide other apps' toasts, and our own window could only be hidden with `WDA_EXCLUDEFROMCAPTURE`.
- Marks drawn in the overlay are not included (it captures the live window).
- The cursor is parked over the area once. **Moving the mouse or pressing Esc stops it** and keeps what's stitched so far. Steps grow to ~1/3 of the area after the first one (sized from the largest distance a notch has moved). Half the area left too little overlap on animated sites. New captures are ignored while it runs.
- Matching (fixed on zaidhossain.com, which stopped after one frame at first):
  - The frame is split into 8 vertical strips. Each strip lists every shift it fits, and the shift the most strips accept wins (at least 2), so an animated card only spoils its own strips. One vote per strip failed on repeating lines (Notepad): every text strip fit any multiple of the line height and outvoted the line-number strip. A lone strip never decides. On a tie, movement beats "didn't move", because a sticky column stays put.
  - Rows match on a tolerant signature (average colour), because browsers re-render text slightly after a scroll.
  - Only 60% of overlapping rows must agree, because scroll effects (text recolouring, images fading in) change about a third of them. Among the near-best shifts, the closest fit wins.
  - When the strips can't agree (a mostly blank overlap, a section that expands as you scroll, a pinned section), it guesses the step: median px per notch so far × notches. The guess is refined by a whole-frame fit that includes blank rows, so section edges pin it down. The refined value is used if at least 80% of rows fit (right ones scored 0.98+); otherwise the plain step is used (refined values at 0.64–0.71 doubled text). At most 10 guesses in a row (a pinned section took 4+). A frame that didn't change at all counts as "didn't move", so a blank end still stops.
  - After each scroll it grabs every ~40 ms until two grabs in a row are alike (a caret or small spinner doesn't count) and the area has moved away from the last frame, up to 700 ms. A fixed 700 ms made every step slow; static pages settle in ~100 ms. "Moved away" stops a scroll that hasn't started yet from passing as the end. (At 350 ms fixed, images were caught half faded in.)
- Known limits: sticky columns repeat down the image, text-reveal animations can be caught mid-way, horizontal carousels come out doubled, the scrollbar thumb shows as marks down the right edge, and scroll-jacked sections (the page holds still, then animates a whole block at once, like the "How I work" timeline on zaidhossain.com) depend on timing: clean in one run, one block doubled in another. Every external stitching tool (ShareX, Snagit, Shottr, CleanShot X…) fails on scroll-driven animation; only in-browser capture avoids it.
- Retested 2026-11-18 on real Chrome windows: a chat page with a fixed composer + spinner + sticky header, 60 messages, 7720 px in 11.7 s, OCR found every message exactly once, composer once at the bottom; 300 near-identical lines, 10854 px in 15.5 s, all 300 once; zaidhossain.com top to bottom in 55–65 s (was ~106 s).
- Tested on a real Notepad with 300 lines: scrolled top to bottom on its own, 700×7398 in ~39 s, slices checked by eye with no seams or doubled lines. Stitch logic has unit tests (sticky header, blank areas, full page).

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

## 11. Status and open items

Released: v0.2.1 on GitHub (installer + auto-update). Every feature in §6 is built and unit- or mock-tested.

Never hand-tested in the real app (the owner's list; tick off here, nowhere else):
- Draw (pen), scrolling capture on a real page, Live Text link chips opening the browser
- Moving, resizing and deleting a blur, the History viewer, the colour palette and custom picker, the light/dark theme
- Save as, Ctrl+Shift+PrtSc text grab, hotkey rebinding, autostart
- Multi-monitor, 125% / 150% display scaling
- AI re-read with a real key

Naming: the visible name is **Shutter** (2026-11-12). The bundle id, exe name and AI-key vault name stay `screenshot-app`, so settings, History and a saved key survive.

## Updates (v0.2.0)
- Auto-update from GitHub Releases: a quiet check 3 s after start, notification if newer; Settings → General has Check / Update (downloads, installs passively, restarts).
- Updates are signed (minisign key); unsigned or tampered installers are refused. Why: an updater is a remote code path, signing is the trust anchor.
- Done in Rust commands, not the JS plugin API, to keep the "everything goes through app commands" rule.

## Owner fixes (2026-11-20)
- Settings always opens on Shortcuts (the first page), not the last page you visited. Owner's call.
- History: click a thumbnail to view the full image in the window (← → step, Ctrl+C / Copy, Esc or click outside closes). Copy stays on the Copy button.
- Redactions (blur, pixelate, black box) can be picked with Select (V): drag to move, Del to delete, Ctrl+Z undoes. Notes stay on top and win a click. Moving re-renders from the original, so the old spot is clean; a redaction stays inside the selection.

## Owner fixes (2026-11-21)
- History images load through the app's own `shot://` protocol (plain file reads, ids checked), not Tauri's asset protocol, which compares canonical paths and refused files Windows had stored elsewhere (virtualized AppData). The asset protocol is now off.
- Finish buttons: **Copy** copies only; **Save** (Enter, primary) saves and copies; Save as also copies once a file is picked. The "Copy & save" button is gone.
- A picked blur / pixelate / black box shows 8 resize handles; edges stop 4 px short of each other so a box never flips; stays inside the selection.
- Colour: 12 swatches (round the wheel, then white, grey, black, brown) plus Custom (the system colour picker), from the Colour button; C still steps through the palette. Step badges pick dark text on light colours.
- Settings: the "General" page is now **Settings** (gear icon) with a Theme row: System / Light / Dark, stored in localStorage `shutter-theme`, applied live to Settings and History.


## macOS and Linux (started 2026-11-24)
Goal: the same app on every Mac (Apple Silicon + Intel) and as many Linux distros as possible, installed with one command. Research summary and decisions:
- **Phases.** 1) code port (done in code, not yet run on a real Mac/Linux box); 2) release CI with `tauri-action` building Windows, one universal macOS app and Linux on ubuntu-22.04 (oldest glibc) into a draft release with one combined `latest.json`; 3) install channels: own Homebrew tap (`brew install --cask zahidhossin39/tap/shutter`), `install.sh` one-liner (deb on Debian/Ubuntu, rpm on Fedora/openSUSE, AppImage elsewhere), AUR `shutter-bin`, winget; Flathub last.
- **Per-OS pieces.** Overlay frame: WebView2 shared memory on Windows, `frame://` protocol elsewhere (raw RGBA, keyed by the asking webview). OCR: Windows.Media.Ocr / Apple Vision / `ocrs` with its ONNX models bundled as Linux resources (`scripts/get-ocr-models.sh`). AI re-read: WinRT HTTP + Credential Manager on Windows, `ureq` + `keyring` (Keychain / Secret Service) elsewhere. Scroll input: SendInput on Windows, `enigo` elsewhere (macOS needs Accessibility; Wayland can't send input, so scrolling capture is X11/macOS only).
- **macOS.** Overlay at screen-saver level on all Spaces, placed in points (xcap reports monitors in points, images in pixels). Accessory activation policy: no Dock icon. Screen Recording permission prompt on first capture. Unsigned for now: users click Privacy & Security > Open Anyway once, and must re-allow Screen Recording after each update (TCC ties grants to the signature). A $99 Developer ID fixes both; owner decides before the Mac release.
- **Linux.** xcap captures on X11 and on Wayland (GNOME Shell screenshot, the xdg portal, or wlroots). Global hotkeys don't work on Wayland: users bind `shutter --capture` in their desktop's keyboard settings. Only the AppImage self-updates; deb/rpm installs must show a "new version" notice instead (to do in phase 2). Tray needs AppIndicator (GNOME needs the extension).
- **Testing.** `check.yml` lints and tests on Windows, macOS and Ubuntu. Nobody here can run the Mac or Linux app; both need a hand-test by a real user before release.
