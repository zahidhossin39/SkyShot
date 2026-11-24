// Capture overlay: one of these runs per monitor, kept hidden until a hotkey fires.
//
// All geometry is in PHYSICAL pixels (the screenshot's own pixels), so selection and
// redaction stay sharp and exact at any Windows display scaling. Mouse positions are
// converted from CSS pixels with `scale()`.

import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { COLORS, drawNote, FONT_PX, isTiny, moved, noteAt, noteBox, type Note, type NoteKind, type Point, type Rect } from "./annotate";
import { findActions, findSensitive, type Action, type Word } from "./detect";
import { recognize, resized, snapAngle, type Shape } from "./ink";

type Mode = "capture" | "text";
type RedactTool = "blur" | "pixelate" | "box";
type Tool = "select" | RedactTool | NoteKind;
const isRedact = (t: Tool): t is RedactTool => t === "blur" || t === "pixelate" || t === "box";
const isNote = (t: Tool): t is NoteKind => t !== "select" && !isRedact(t);
type Handle = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";
type OpenPayload = { mode: Mode; width: number; height: number };
type Op = { tool: RedactTool; rects: Rect[] };
type Pick = { rect: Rect; on: boolean };
/** Smart Redact in progress: every OCR word (click to add) and the suggested boxes. */
type Smart = { words: Rect[]; picks: Pick[]; loading: boolean; error?: string };
type Drag =
  | { kind: "new"; origin: Point; snap: Rect | null } // snap: window to select if it's just a click
  | { kind: "move"; origin: Point; start: Rect }
  | { kind: "resize"; origin: Point; start: Rect; handle: Handle }
  | { kind: "op"; origin: Point }
  | { kind: "note"; origin: Point } // drawing the last note in `notes`
  | { kind: "notemove"; origin: Point; start: Note }
  | { kind: "opmove"; origin: Point; start: Rect; handle?: Handle } // handle: resizing, not moving
  // Drawing the last note in `notes` with the pen. at: pointer now. still: where it last came to
  // rest (for hold-to-snap). snap: the shape it snapped to, resized while you keep dragging.
  | { kind: "pen"; origin: Point; at: Point; still: Point; straight?: boolean; snap?: { shape: Shape; from: Point } };

const ACCENT = "#3b82f6";
const MIN_SIZE = 3;

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const view = $<HTMLCanvasElement>("view");
const ui = $<HTMLCanvasElement>("ui");
const vctx = view.getContext("2d")!;
const uctx = ui.getContext("2d")!;
const hint = $("hint");
const sizeLabel = $("size");
const toolbar = $("toolbar");
const panel = $("textpanel");
const textOut = $<HTMLTextAreaElement>("text-out");
const textStatus = $("text-status");
const liveLayer = $("livetext");
const textActions = $("text-actions");
const smartbar = $("smartbar");
const smartStatus = $("smart-status");
const smartApply = $<HTMLButtonElement>("smart-apply");
const noteInput = $<HTMLTextAreaElement>("note-input");
const loupe = $("loupe");
const loupeCanvas = $<HTMLCanvasElement>("loupe-canvas");
const lctx = loupeCanvas.getContext("2d")!;
const tmp = document.createElement("canvas");
const tctx = tmp.getContext("2d")!;
/** Screenshot + redactions, without annotations, so notes can be redrawn on every mouse move. */
const base = document.createElement("canvas");
const bctx = base.getContext("2d")!;

let mode: Mode = "capture";
let W = 0;
let H = 0;
let original: ImageData | null = null;
let opened = false;
let busy = false;
let sel: Rect | null = null;
let tool: Tool = "select";
let ops: Op[] = [];
let drag: Drag | null = null;
let pendingOp: Rect | null = null;
let smart: Smart | null = null;
/** Live Text words, per OCR line, while the text panel is open. */
let live: { el: HTMLElement; text: string }[][] | null = null;
let notes: Note[] = [];
let picked = -1; // selected note, for move / delete / recolour
/** Selected redaction (one rect of one op), for move / delete. Moving re-renders from the
 * original, so the old spot is clean again (nothing burned in until you finish). */
let pickedOp: { op: number; rect: number } | null = null;
let color = COLORS[0];
let penSize = 5; // CSS px
let penAlpha = 1;
/** Where the pen would touch down (physical px), for the size ring; null while drawing or away. */
let penHover: Point | null = null;
const penSizeInput = $<HTMLInputElement>("pen-size");
const penAlphaInput = $<HTMLInputElement>("pen-alpha");
/** Text note being typed: where, and which note it replaces (-1 = new). */
let editing: { at: Point; idx: number } | null = null;
/** Undo snapshots. Covers redactions and every annotation change (add, move, edit, delete). */
let history: { ops: Op[]; notes: Note[] }[] = [];
/** Visible windows on this monitor, top-most first (from Rust, shortly after open). */
let windows: Rect[] = [];
let hoverWin: Rect | null = null;
let lastPoint: Point | null = null;
let hex = "";

/** Physical pixels per CSS pixel. */
const scale = () => (W ? W / window.innerWidth : window.devicePixelRatio);

function toPhys(e: MouseEvent): Point {
  const s = scale();
  return {
    x: Math.max(0, Math.min(W, Math.round(e.clientX * s))),
    y: Math.max(0, Math.min(H, Math.round(e.clientY * s))),
  };
}

function rectFrom(a: Point, b: Point): Rect {
  const x = Math.min(a.x, b.x);
  const y = Math.min(a.y, b.y);
  return { x, y, w: Math.abs(a.x - b.x), h: Math.abs(a.y - b.y) };
}

function clampTo(r: Rect, bounds: Rect): Rect {
  const x = Math.max(bounds.x, r.x);
  const y = Math.max(bounds.y, r.y);
  const right = Math.min(bounds.x + bounds.w, r.x + r.w);
  const bottom = Math.min(bounds.y + bounds.h, r.y + r.h);
  return { x, y, w: Math.max(0, right - x), h: Math.max(0, bottom - y) };
}

const inside = (p: Point, r: Rect) =>
  p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h;

function handleAt(p: Point, r: Rect): Handle | null {
  const t = 6 * scale();
  const near = (a: number, b: number) => Math.abs(a - b) <= t;
  const inX = p.x >= r.x - t && p.x <= r.x + r.w + t;
  const inY = p.y >= r.y - t && p.y <= r.y + r.h + t;
  const n = near(p.y, r.y) && inX;
  const s = near(p.y, r.y + r.h) && inX;
  const w = near(p.x, r.x) && inY;
  const e = near(p.x, r.x + r.w) && inY;
  if (n && w) return "nw";
  if (n && e) return "ne";
  if (s && w) return "sw";
  if (s && e) return "se";
  if (n) return "n";
  if (s) return "s";
  if (w) return "w";
  if (e) return "e";
  return null;
}

const CURSORS: Record<Handle, string> = {
  n: "ns-resize", s: "ns-resize", e: "ew-resize", w: "ew-resize",
  ne: "nesw-resize", sw: "nesw-resize", nw: "nwse-resize", se: "nwse-resize",
};

// ---------------------------------------------------------------- redaction

/** Burn a redaction into the view canvas. Pixels underneath are destroyed. */
const applyOp = (op: Op) => op.rects.forEach((r) => applyRect(op.tool, r));

function applyRect(tool: RedactTool, { x, y, w, h }: Rect) {
  if (tool === "box") {
    vctx.fillStyle = "#000";
    vctx.fillRect(x, y, w, h);
    return;
  }

  if (tool === "pixelate") {
    const block = Math.max(8, Math.min(32, Math.round(Math.min(w, h) / 5)));
    const sw = Math.max(1, Math.ceil(w / block));
    const sh = Math.max(1, Math.ceil(h / block));
    tmp.width = sw;
    tmp.height = sh;
    tctx.imageSmoothingEnabled = true;
    tctx.imageSmoothingQuality = "high";
    tctx.drawImage(view, x, y, w, h, 0, 0, sw, sh);
    vctx.imageSmoothingEnabled = false;
    vctx.drawImage(tmp, 0, 0, sw, sh, x, y, w, h);
    vctx.imageSmoothingEnabled = true;
    return;
  }

  // Blur: copy the area plus a margin, blur it, draw it back clipped to the rect.
  const radius = Math.max(8, Math.min(48, Math.round(Math.min(w, h) / 3)));
  const m = radius * 2;
  tmp.width = w + 2 * m;
  tmp.height = h + 2 * m;
  tctx.drawImage(view, x - m, y - m, tmp.width, tmp.height, 0, 0, tmp.width, tmp.height);
  // Near screen edges the margin is empty (transparent), which would let the original
  // pixels show through the blur. Underpaint with the area's average colour first.
  const avg = document.createElement("canvas");
  avg.width = avg.height = 1;
  const actx = avg.getContext("2d")!;
  actx.imageSmoothingQuality = "high";
  actx.drawImage(view, x, y, w, h, 0, 0, 1, 1);
  vctx.save();
  vctx.beginPath();
  vctx.rect(x, y, w, h);
  vctx.clip();
  vctx.drawImage(avg, x, y, w, h);
  vctx.filter = `blur(${radius}px)`;
  vctx.drawImage(tmp, x - m, y - m);
  vctx.restore();
}

/** Redactions changed: rebuild the base (original + redactions), then the notes on top. */
function renderImage() {
  if (!original) return;
  vctx.putImageData(original, 0, 0);
  ops.forEach(applyOp);
  bctx.drawImage(view, 0, 0);
  drawNotes();
}

let imageQueued = false;
/** renderImage at most once a frame (re-blurring every op on each mouse move is too much). */
function renderImageSoon() {
  if (imageQueued) return;
  imageQueued = true;
  requestAnimationFrame(() => {
    imageQueued = false;
    renderImage();
  });
}

/** Topmost redaction rect under `p`. */
function opAt(p: Point): { op: number; rect: number } | null {
  for (let i = ops.length - 1; i >= 0; i--) {
    for (let j = ops[i].rects.length - 1; j >= 0; j--) if (inside(p, ops[i].rects[j])) return { op: i, rect: j };
  }
  return null;
}

const pickedRect = () => (pickedOp ? ops[pickedOp.op]?.rects[pickedOp.rect] : undefined);

/** Replaces one rect; a new op object, so undo snapshots (shallow copies of `ops`) keep the old one. */
function setOpRect(at: { op: number; rect: number }, r: Rect) {
  const op = ops[at.op];
  ops[at.op] = { ...op, rects: op.rects.map((x, j) => (j === at.rect ? r : x)) };
}

/** Annotations changed: base + every note. Cheap, so it runs on every mouse move. */
function drawNotes() {
  vctx.drawImage(base, 0, 0);
  const s = scale();
  let step = 0;
  notes.forEach((n, i) => i !== editing?.idx && drawNote(vctx, n, s, n.kind === "step" ? ++step : 0));
}

const snapshot = () => history.push({ ops: [...ops], notes: structuredClone(notes) });

/** Burn a new redaction into the base, under the annotations (so arrows never get blurred). */
function addRedaction(op: Op) {
  snapshot();
  ops.push(op);
  vctx.drawImage(base, 0, 0);
  applyOp(op);
  bctx.drawImage(view, 0, 0);
  drawNotes();
}

function undo() {
  const prev = history.pop();
  if (!prev) return;
  const redactionsChanged = prev.ops.length !== ops.length || prev.ops.some((o, i) => o !== ops[i]);
  ({ ops, notes } = prev);
  picked = -1;
  pickedOp = null;
  if (redactionsChanged) renderImage();
  else drawNotes();
  updateChrome();
  renderUI();
}

// ---------------------------------------------------------------- drawing UI

function renderUI() {
  uctx.clearRect(0, 0, W, H);
  uctx.fillStyle = "rgba(0, 0, 0, 0.45)";
  uctx.fillRect(0, 0, W, H);
  const s = scale();
  // Snap preview: outline the window under the cursor (one click selects it). The screen stays
  // dim until you actually select, like Lightshot; un-dimming a maximised window lit up everything.
  if (!sel && hoverWin) {
    const r = hoverWin;
    uctx.strokeStyle = ACCENT;
    uctx.lineWidth = Math.max(2, Math.round(2 * s));
    uctx.strokeRect(r.x + uctx.lineWidth / 2, r.y + uctx.lineWidth / 2, r.w - uctx.lineWidth, r.h - uctx.lineWidth);
  }
  if (!sel || sel.w === 0 || sel.h === 0) return;

  uctx.clearRect(sel.x, sel.y, sel.w, sel.h);
  uctx.strokeStyle = ACCENT;
  uctx.lineWidth = Math.max(1, Math.round(s));
  uctx.strokeRect(sel.x + 0.5, sel.y + 0.5, sel.w - 1, sel.h - 1);

  if (mode === "capture" && drag?.kind !== "new" && !smart) {
    const hs = Math.round(6 * s);
    const cx = [sel.x, sel.x + sel.w / 2, sel.x + sel.w];
    const cy = [sel.y, sel.y + sel.h / 2, sel.y + sel.h];
    uctx.fillStyle = "#fff";
    for (const x of cx) {
      for (const y of cy) {
        if (x === cx[1] && y === cy[1]) continue;
        uctx.fillRect(x - hs / 2, y - hs / 2, hs, hs);
        uctx.strokeRect(x - hs / 2 + 0.5, y - hs / 2 + 0.5, hs - 1, hs - 1);
      }
    }
  }

  if (pendingOp && pendingOp.w > 0 && pendingOp.h > 0) {
    uctx.fillStyle = tool === "box" ? "rgba(0, 0, 0, 0.6)" : "rgba(255, 255, 255, 0.18)";
    uctx.fillRect(pendingOp.x, pendingOp.y, pendingOp.w, pendingOp.h);
    uctx.setLineDash([4 * s, 3 * s]);
    uctx.strokeStyle = "#fff";
    uctx.strokeRect(pendingOp.x + 0.5, pendingOp.y + 0.5, pendingOp.w - 1, pendingOp.h - 1);
    uctx.setLineDash([]);
  }

  // Pen size ring around the exact touch-down point (same pixels as the ink, so it can't drift).
  if (penHover && !drag) {
    const r = Math.max(2 * s, (penSize * s) / 2);
    uctx.lineWidth = Math.max(1, s);
    uctx.strokeStyle = "rgba(0, 0, 0, 0.55)";
    uctx.beginPath();
    uctx.arc(penHover.x, penHover.y, r + uctx.lineWidth, 0, Math.PI * 2);
    uctx.stroke();
    uctx.strokeStyle = color;
    uctx.beginPath();
    uctx.arc(penHover.x, penHover.y, r, 0, Math.PI * 2);
    uctx.stroke();
  }

  const pr = pickedRect();
  if (pr) {
    const pd = 3 * s;
    uctx.setLineDash([4 * s, 3 * s]);
    uctx.strokeStyle = ACCENT;
    uctx.lineWidth = Math.max(1, Math.round(1.5 * s));
    uctx.strokeRect(pr.x - pd, pr.y - pd, pr.w + 2 * pd, pr.h + 2 * pd);
    uctx.setLineDash([]);
    // Resize handles on its corners and edges (drawn like the selection's).
    const hs = Math.round(6 * s);
    uctx.fillStyle = "#fff";
    uctx.lineWidth = Math.max(1, Math.round(s));
    for (const x of [pr.x, pr.x + pr.w / 2, pr.x + pr.w]) {
      for (const y of [pr.y, pr.y + pr.h / 2, pr.y + pr.h]) {
        if (x === pr.x + pr.w / 2 && y === pr.y + pr.h / 2) continue;
        uctx.fillRect(x - hs / 2, y - hs / 2, hs, hs);
        uctx.strokeRect(x - hs / 2 + 0.5, y - hs / 2 + 0.5, hs - 1, hs - 1);
      }
    }
  }

  if (notes[picked]) {
    const b = noteBox(vctx, notes[picked], s);
    const pd = 5 * s;
    uctx.setLineDash([4 * s, 3 * s]);
    uctx.strokeStyle = ACCENT;
    uctx.strokeRect(b.x - pd, b.y - pd, b.w + 2 * pd, b.h + 2 * pd);
    uctx.setLineDash([]);
  }

  // Smart Redact suggestions: red = will be hidden, faint = kept visible.
  if (smart) {
    uctx.setLineDash([4 * s, 3 * s]);
    for (const { rect: r, on } of smart.picks) {
      uctx.fillStyle = on ? "rgba(239, 68, 68, 0.28)" : "rgba(255, 255, 255, 0.06)";
      uctx.fillRect(r.x, r.y, r.w, r.h);
      uctx.strokeStyle = on ? "#f87171" : "rgba(255, 255, 255, 0.6)";
      uctx.strokeRect(r.x + 0.5, r.y + 0.5, r.w - 1, r.h - 1);
    }
    uctx.setLineDash([]);
  }
}

/** Position the DOM bits (hint, size label, toolbar) around the selection. */
function updateChrome() {
  const s = scale();
  const has = !!sel && sel.w >= MIN_SIZE && sel.h >= MIN_SIZE;

  setHint(
    smart
      ? `Click a box to keep it visible${SEP}Click any word to hide it${SEP}<kbd>Enter</kbd> redact${SEP}<kbd>Esc</kbd> back`
      : live
      ? `Drag over the image to select text${SEP}<kbd>Ctrl+C</kbd> copy${SEP}<kbd>Esc</kbd> close`
      : !sel
      ? `${mode === "text" ? "Drag over text to copy it" : "Drag to select an area"}${SEP}Click a window to grab it${SEP}<kbd>Z</kbd> ${loupeOn ? "hide zoom" : "zoom"}${loupeOn ? `${SEP}<kbd>C</kbd> copy colour` : ""}${SEP}<kbd>Esc</kbd> cancel`
      : editing
      ? `Type your text${SEP}<kbd>Enter</kbd> done${SEP}<kbd>Shift+Enter</kbd> new line${SEP}<kbd>Esc</kbd> discard`
      : pickedRect() && tool === "select"
      ? `Drag to move${SEP}<kbd>Del</kbd> delete`
      : notes[picked] && tool === "select"
      ? `Drag to move${SEP}<kbd>Del</kbd> delete${SEP}<kbd>C</kbd> colour${SEP}Double-click text to edit`
      : has && mode === "capture" && tool !== "select"
        ? `${TOOL_HINTS[tool]}${SEP}<kbd>Ctrl+Z</kbd> undo${SEP}<kbd>V</kbd> done`
        : null,
  );

  sizeLabel.hidden = !has;
  if (has && sel) {
    sizeLabel.textContent = `${sel.w} × ${sel.h}`;
    const top = sel.y / s - 24;
    sizeLabel.style.left = `${sel.x / s}px`;
    sizeLabel.style.top = `${top < 4 ? sel.y / s + 4 : top}px`;
  }

  smartbar.hidden = !smart;
  if (smart && sel) {
    const on = smart.picks.filter((k) => k.on).length;
    smartbar.classList.toggle("loading", smart.loading);
    smartStatus.textContent = smart.loading
      ? "Looking for sensitive text…"
      : smart.error ??
        (smart.picks.length ? `${on} of ${smart.picks.length} selected` : "Nothing found. Click words to hide them.");
    smartApply.textContent = on ? `Redact ${on}` : "Redact";
    smartApply.disabled = smart.loading || on === 0;
    placeNear(smartbar, sel);
  }

  const showBar = has && mode === "capture" && !drag && !smart;
  toolbar.hidden = !showBar;
  if (!showBar) hideTip();
  if (showBar && sel) {
    toolbar.querySelectorAll<HTMLButtonElement>("[data-tool]").forEach((b) =>
      b.classList.toggle("active", b.dataset.tool === tool),
    );
    toolbar.querySelector<HTMLButtonElement>('[data-action="undo"]')!.disabled = history.length === 0;
    toolbar.querySelector<HTMLElement>(".swatch")!.style.background = color;
    const opts = toolbar.querySelector<HTMLElement>(".pen-opts")!;
    opts.hidden = colorOpen || (tool !== "pen" && notes[picked]?.kind !== "pen");
    colorPop.hidden = !colorOpen;
    colorPop.querySelectorAll<HTMLElement>("[data-color]").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.color === color)));
    if (/^#[0-9a-f]{6}$/i.test(color)) colorCustom.value = color;
    penSizeInput.value = String(penSize);
    penAlphaInput.value = String(Math.round(penAlpha * 100));
    $("pen-size-val").textContent = String(penSize);
    $("pen-alpha-val").textContent = `${Math.round(penAlpha * 100)}%`;
    placeNear(toolbar, sel);
    // Panels sit away from the selection when there's room, so they don't cover the image.
    const tb = toolbar.getBoundingClientRect();
    const place = (el: HTMLElement, anchor: string, height: number) => {
      const below = tb.top >= (sel!.y + sel!.h) / s ? tb.bottom + height < window.innerHeight : tb.top < height;
      el.classList.toggle("below", below);
      el.style.left = `${toolbar.querySelector<HTMLElement>(anchor)!.getBoundingClientRect().left - tb.left}px`;
    };
    if (!opts.hidden) place(opts, '[data-tool="pen"]', 48);
    if (colorOpen) place(colorPop, '[data-action="color"]', 96);
  }
  if (!panel.hidden && sel) placePanel();
}

const SEP = '<span class="sep">·</span>';
const TOOL_HINTS: Record<Exclude<Tool, "select">, string> = {
  blur: "Drag inside the selection to blur",
  pixelate: "Drag inside the selection to pixelate",
  box: "Drag inside the selection to black it out",
  arrow: "Drag to draw an arrow",
  rect: "Drag to draw a box",
  mark: "Drag over text to highlight it",
  text: "Click to add text",
  pen: `Draw freely${SEP}Hold still to snap to a line, circle or box${SEP}<kbd>Shift</kbd> straight${SEP}<kbd>[</kbd> <kbd>]</kbd> size`,
  step: "Click to add numbered steps (1, 2, 3…)",
};

let hintHtml: string | null = null;
/** Top-centre pill. Fixed strings only, so innerHTML is safe. */
function setHint(html: string | null) {
  hint.hidden = !html;
  if (html && html !== hintHtml) hint.innerHTML = html;
  hintHtml = html;
}

// Hover tooltip: name, shortcut and what the button does. The first hover waits a
// beat; sliding along the bar after that shows each tip instantly.
const tip = $("tip");
let tipTimer = 0;
let tipHiddenAt = 0;

function showTip(btn: HTMLElement) {
  $("tip-name").textContent = btn.dataset.name ?? "";
  $("tip-key").textContent = btn.dataset.key ?? "";
  $("tip-desc").textContent = btn.dataset.desc ?? "";
  tip.hidden = false;
  const b = btn.getBoundingClientRect();
  const bar = toolbar.getBoundingClientRect();
  const left = Math.max(8, Math.min(window.innerWidth - tip.offsetWidth - 8, b.left + b.width / 2 - tip.offsetWidth / 2));
  let top = bar.top - tip.offsetHeight - 8;
  if (top < 8) top = bar.bottom + 8;
  tip.style.left = `${left}px`;
  tip.style.top = `${top}px`;
}

function hideTip() {
  clearTimeout(tipTimer);
  if (!tip.hidden) tipHiddenAt = performance.now();
  tip.hidden = true;
}

/** Below the selection, right-aligned; flips above, then inside, if out of room. */
function placeNear(el: HTMLElement, r: Rect) {
  const s = scale();
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const bw = el.offsetWidth;
  const bh = el.offsetHeight;
  const left = Math.max(8, Math.min(vw - bw - 8, (r.x + r.w) / s - bw));
  let top = (r.y + r.h) / s + 8;
  if (top + bh > vh - 8) top = r.y / s - bh - 8;
  if (top < 8) top = (r.y + r.h) / s - bh - 8;
  el.style.left = `${left}px`;
  el.style.top = `${Math.max(8, top)}px`;
}

function placePanel() {
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const tb = toolbar.getBoundingClientRect();
  const pw = panel.offsetWidth;
  const ph = panel.offsetHeight;
  const left = Math.max(8, Math.min(vw - pw - 8, tb.right - pw));
  let top = tb.bottom + 8;
  if (top + ph > vh - 8) top = tb.top - ph - 8;
  panel.style.left = `${left}px`;
  panel.style.top = `${Math.max(8, top)}px`;
}

// ---------------------------------------------------------------- open / close

const webview = (window as unknown as { chrome: { webview: EventTarget & { releaseBuffer(b: ArrayBuffer): void } } })
  .chrome.webview;

/** Rust posts the frame as WebView2 shared memory (zero-copy). Copy it out and release it. */
async function receiveFrame(): Promise<ArrayBuffer> {
  const got = new Promise<ArrayBuffer>((resolve) =>
    webview.addEventListener(
      "sharedbufferreceived",
      (e) => {
        const shared = (e as Event & { getBuffer(): ArrayBuffer }).getBuffer();
        const copy = shared.slice(0); // the shared one is read-only and must be released
        webview.releaseBuffer(shared);
        resolve(copy);
      },
      { once: true },
    ),
  );
  await invoke("overlay_frame");
  return got;
}

async function open(p: OpenPayload) {
  if (opened) return;
  opened = true;
  try {
    mode = p.mode;
    W = p.width;
    H = p.height;
    view.width = ui.width = base.width = W;
    view.height = ui.height = base.height = H;
    original = new ImageData(new Uint8ClampedArray(await receiveFrame()), W, H);
    vctx.putImageData(original, 0, 0);
    bctx.putImageData(original, 0, 0);
    sel = null;
    ops = [];
    notes = [];
    history = [];
    picked = -1;
    pickedOp = null;
    smart = null;
    tool = "select";
    renderUI();
    updateChrome();
    // Show only once the frozen frame is on screen (else Windows shows the old/blank surface
    // for a frame). The dim layer starts invisible, so the first visible frame is identical to
    // the real screen; then it fades in.
    await painted();
    const ms = await invoke<number>("overlay_shown");
    requestAnimationFrame(() => document.body.classList.remove("pre"));
    console.log(`[overlay] visible ${ms} ms after hotkey`);
  } catch (e) {
    console.error("[overlay] open failed", e);
    opened = false;
  }
}

function reset() {
  document.body.classList.remove("leaving");
  document.body.classList.add("pre"); // next open starts with the dim invisible
  opened = false;
  busy = false;
  original = null;
  sel = null;
  ops = [];
  drag = null;
  pendingOp = null;
  smart = null;
  notes = [];
  history = [];
  picked = -1;
  pickedOp = null;
  colorOpen = false;
  editing = null;
  windows = [];
  hoverWin = null;
  closeText();
  toolbar.hidden = smartbar.hidden = sizeLabel.hidden = noteInput.hidden = loupe.hidden = true;
  loupeOn = false;
  setHint(null);
  hideTip();
  // Shrink canvases so a hidden overlay doesn't hold a full screenshot in memory.
  view.width = view.height = ui.width = ui.height = base.width = base.height = 1;
  W = H = 0;
}

/** Two frames: the next paint has been presented. Capped, since a hidden window may not paint. */
const painted = () =>
  Promise.race([
    new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))),
    new Promise((r) => setTimeout(r, 50)),
  ]);

const LEAVE_MS = 110;
/** Fade the dim + toolbars out over the frozen frame before the window hides, so closing
 * doesn't snap. Only the chrome fades: the frozen screenshot underneath matches the real screen. */
async function leave() {
  hideTip();
  document.body.classList.add("leaving");
  await new Promise((r) => setTimeout(r, LEAVE_MS));
}

/** Fade out, then hide the window before any pixels are sent (see capture::hide). */
async function leaveAndHide(fade = true) {
  if (fade) await leave();
  await invoke("overlay_hide");
}

/** A hidden overlay whose hand-off failed would leave the session stuck (hotkeys ignored). */
const failed = (what: string) => (e: unknown) => {
  console.error(`[overlay] ${what} failed`, e);
  busy = false;
  void invoke("overlay_close");
};

const close = async () => {
  await leave();
  return invoke("overlay_close");
};

/** Selected area as raw RGBA, ready to send to Rust. */
function cropSelection() {
  const r = sel!;
  const img = vctx.getImageData(r.x, r.y, r.w, r.h);
  return {
    bytes: new Uint8Array(img.data.buffer),
    headers: { "x-width": String(img.width), "x-height": String(img.height) },
  };
}

async function finish(action: "copy" | "save" | "both" | "saveas") {
  if (!sel || busy) return;
  busy = true;
  const { bytes, headers } = cropSelection();
  // Save as opens a dialog, so it can't wait on a fade first; the rest fade out.
  await leaveAndHide(action !== "saveas");
  await invoke("finish", bytes, { headers: { ...headers, "x-action": action } }).catch(failed("finish"));
}

/** Pinning (also add finished shots to the shelf): Rust keeps the setting, the tray shares it. */
function showPinning(on: boolean) {
  toolbar.querySelector('[data-action="pinning"]')?.setAttribute("aria-pressed", String(on));
}
const togglePinning = () =>
  invoke<boolean>("get_pinning")
    .then((on) => invoke("set_pinning", { enabled: !on }))
    .catch(failed("pinning"));
void invoke<boolean>("get_pinning").then(showPinning, () => {});
void getCurrentWebviewWindow().listen<boolean>("pinning:changed", (e) => showPinning(e.payload));

async function scrollCapture() {
  if (!sel || busy) return;
  busy = true;
  await leave();
  try {
    await invoke("scroll_capture", { x: sel.x, y: sel.y, w: sel.w, h: sel.h });
  } catch (e) {
    console.error("[overlay] scroll capture failed", e);
    busy = false;
  }
}

async function grabText() {
  if (!sel || busy) return;
  busy = true;
  const { bytes, headers } = cropSelection();
  await leaveAndHide();
  await invoke("grab_text", bytes, { headers }).catch(failed("grab_text"));
}

/** Text panel + Live Text + Smart actions, all from one OCR pass. */
async function extractText() {
  if (!sel || busy) return;
  const r = sel;
  closeText();
  tool = "select"; // Live Text needs the mouse; redaction tools would fight it
  panel.hidden = false;
  panel.classList.add("loading");
  textStatus.textContent = "Reading text…";
  textOut.value = "";
  updateChrome();
  const { bytes, headers } = cropSelection();
  try {
    const { text, lines } = await invoke<{ text: string; lines: Word[][] }>("ocr_image", bytes, { headers });
    if (panel.hidden || sel !== r) return; // closed or reselected meanwhile
    textOut.value = text;
    const words = text.split(/\s+/).filter(Boolean).length;
    textStatus.textContent = text.trim() ? `${words} words found — fix anything, then copy` : "No text found in this area";
    renderActions(findActions(text));
    showLive(lines, r);
    updateChrome();
  } catch (e) {
    textStatus.textContent = String(e);
  } finally {
    panel.classList.remove("loading");
  }
}

function closeText() {
  panel.hidden = true;
  textActions.replaceChildren();
  live = null;
  liveLayer.hidden = true;
  liveLayer.replaceChildren();
  getSelection()?.removeAllRanges();
}

/** Better read of the same area by the user's AI provider. Replaces the panel text only. */
async function aiReread() {
  if (!sel || busy) return;
  const r = sel;
  const btn = $<HTMLButtonElement>("text-ai");
  btn.disabled = true;
  panel.classList.add("loading");
  textStatus.textContent = "AI is reading this area…";
  const { bytes, headers } = cropSelection(); // already redacted: hidden text is never sent
  try {
    const text = await invoke<string>("ai_reread", bytes, { headers });
    if (panel.hidden || sel !== r) return;
    textOut.value = text;
    textStatus.textContent = text ? "Re-read by AI — check it, then copy" : "AI found no text in this area";
  } catch (e) {
    textStatus.textContent = String(e);
  } finally {
    panel.classList.remove("loading");
    btn.disabled = false;
  }
}

async function copyValue(text: string) {
  try {
    await invoke("copy_text", { text });
    textStatus.textContent = "Copied ✓";
  } catch (e) {
    textStatus.textContent = String(e);
  }
}

const ACTION_VERB: Record<Action["kind"], string> = { url: "Open", email: "Copy", phone: "Copy" };

/** Smart actions: one-click chips for links, emails and phone numbers found in the text. */
function renderActions(actions: Action[]) {
  textActions.replaceChildren(
    ...actions.slice(0, 3).map((a) => {
      const b = document.createElement("button");
      b.className = "chip";
      b.title = a.value;
      b.textContent = `${ACTION_VERB[a.kind]} ${a.value.replace(/^https?:\/\//i, "")}`;
      b.onclick = () =>
        a.kind === "url"
          ? invoke("open_url", { url: a.value }).catch((e) => (textStatus.textContent = String(e)))
          : copyValue(a.value);
      return b;
    }),
  );
}

// ---------------------------------------------------------------- live text

/** Invisible, selectable copy of the OCR words laid over the image (like a PDF text layer). */
function showLive(lines: Word[][], r: Rect) {
  const s = scale();
  Object.assign(liveLayer.style, { left: `${r.x / s}px`, top: `${r.y / s}px`, width: `${r.w / s}px`, height: `${r.h / s}px` });
  const boxes: number[] = [];
  live = lines.map((line) =>
    line.map((w) => {
      const el = document.createElement("span");
      el.textContent = w.text;
      Object.assign(el.style, { left: `${w.x / s}px`, top: `${w.y / s}px`, fontSize: `${w.h / s}px` });
      liveLayer.append(el);
      boxes.push(w.w / s);
      return { el, text: w.text };
    }),
  );
  liveLayer.hidden = false;
  // Stretch each word to its box so the highlight sits on the image's text. Read all widths
  // first, then write, so layout runs once.
  const els = live.flat().map((x) => x.el);
  const widths = els.map((el) => el.offsetWidth);
  els.forEach((el, i) => (el.style.transform = `scaleX(${widths[i] ? boxes[i] / widths[i] : 1})`));
}

// The layer's spans are positioned, so the browser's own copy would come out jumbled.
// Rebuild it from OCR lines instead: whole words, spaces between, one line per row.
document.addEventListener("copy", (e) => {
  const s = getSelection();
  if (!live || !s || s.isCollapsed || e.target === textOut) return;
  const text = live
    .map((line) => line.filter((w) => s.containsNode(w.el, true)).map((w) => w.text).join(" "))
    .filter(Boolean)
    .join("\n");
  e.clipboardData?.setData("text/plain", text);
  e.preventDefault();
  textStatus.textContent = "Copied ✓";
});

// ---------------------------------------------------------------- smart redact

/** OCR box (relative to the selection) -> padded screen rect inside the selection. */
function boxToRect(b: Rect, r: Rect): Rect {
  const p = 3 * scale();
  const x = Math.floor(r.x + b.x - p);
  const y = Math.floor(r.y + b.y - p);
  return clampTo({ x, y, w: Math.ceil(r.x + b.x + b.w + p) - x, h: Math.ceil(r.y + b.y + b.h + p) - y }, r);
}

function union(ws: Rect[]): Rect {
  const x = Math.min(...ws.map((w) => w.x));
  const y = Math.min(...ws.map((w) => w.y));
  return { x, y, w: Math.max(...ws.map((w) => w.x + w.w)) - x, h: Math.max(...ws.map((w) => w.y + w.h)) - y };
}

async function startSmart() {
  if (!sel || busy || smart) return;
  const r = sel;
  const s: Smart = { words: [], picks: [], loading: true };
  smart = s;
  pickedOp = null;
  closeText();
  updateChrome();
  renderUI();
  const { bytes, headers } = cropSelection();
  try {
    const { lines } = await invoke<{ lines: Word[][] }>("ocr_image", bytes, { headers });
    s.words = lines.flat().map((w) => boxToRect(w, r));
    s.picks = findSensitive(lines).map((f) => ({ rect: boxToRect(union(f.words), r), on: true }));
  } catch (e) {
    s.error = String(e);
  }
  s.loading = false;
  if (smart !== s) return; // cancelled, or the overlay closed meanwhile
  updateChrome();
  renderUI();
}

/** Click a suggestion to toggle it; click any other word to add it. */
function smartClick(p: Point) {
  const s = smart!;
  const pick = s.picks.find((k) => inside(p, k.rect));
  if (pick) pick.on = !pick.on;
  else {
    const word = s.words.find((w) => inside(p, w));
    if (word) s.picks.push({ rect: word, on: true });
  }
}

const overWord = (p: Point) => !!smart && [...smart.picks.map((k) => k.rect), ...smart.words].some((r) => inside(p, r));

/** One op for the whole batch, so a single undo brings it all back. */
function applySmart() {
  if (!smart || smart.loading) return;
  const rects = smart.picks.filter((k) => k.on).map((k) => k.rect);
  smart = null;
  if (rects.length) {
    // Black box unless the user picked blur/pixelate first: safest for keys and passwords.
    addRedaction({ tool: isRedact(tool) ? tool : "box", rects });
  }
  updateChrome();
  renderUI();
}

function cancelSmart() {
  smart = null;
  updateChrome();
  renderUI();
}

// ---------------------------------------------------------------- annotations

function newNote(kind: Exclude<NoteKind, "text" | "pen">, p: Point): Note {
  if (kind === "arrow") return { kind, color, a: p, b: p };
  if (kind === "step") return { kind, color, at: p };
  return { kind, color, r: { x: p.x, y: p.y, w: 0, h: 0 } };
}

/** Text notes are typed into a real textarea at the click, then drawn into the image. */
function openNoteInput(at: Point, idx = -1) {
  const s = scale();
  const old = notes[idx];
  if (old) color = old.color;
  editing = { at, idx };
  noteInput.value = old?.kind === "text" ? old.text : "";
  Object.assign(noteInput.style, { left: `${at.x / s}px`, top: `${at.y / s}px`, color, fontSize: `${FONT_PX}px` });
  noteInput.hidden = false;
  noteInput.focus();
  drawNotes(); // hides the note being edited, so it isn't shown twice
  updateChrome();
}

function commitNoteInput(keep = true) {
  if (!editing) return;
  const { at, idx } = editing;
  const text = noteInput.value.replace(/\s+$/, "");
  editing = null;
  noteInput.hidden = true;
  const old = notes[idx];
  const unchanged = old?.kind === "text" ? old.text === text && old.color === color : !text;
  if (keep && !unchanged) {
    snapshot();
    if (!text) notes.splice(idx, 1);
    else if (idx >= 0) notes[idx] = { kind: "text", color, at, text };
    else notes.push({ kind: "text", color, at, text });
    picked = -1;
  }
  drawNotes();
  updateChrome();
  renderUI();
}

/** New colour for new marks, and for the picked note. `snap` false while dragging around the
 * custom picker, so the whole drag is one undo step. */
function setColor(c: string, snap = true) {
  color = c;
  if (editing) noteInput.style.color = color;
  else if (notes[picked]) {
    if (snap) snapshot();
    notes[picked] = { ...notes[picked], color };
    drawNotes();
  }
  updateChrome();
}

function cycleColor() {
  setColor(COLORS[(COLORS.indexOf(color) + 1) % COLORS.length]);
}

// ---------------------------------------------------------------- colour palette
const colorPop = toolbar.querySelector<HTMLElement>(".color-pop")!;
const colorCustom = $<HTMLInputElement>("color-custom");
let colorOpen = false;
toolbar.querySelector(".color-grid")!.append(
  ...COLORS.map((c) => {
    const b = Object.assign(document.createElement("button"), { title: c });
    b.style.background = c;
    b.dataset.color = c;
    b.setAttribute("aria-label", c);
    return b;
  }),
);
colorPop.addEventListener("click", (e) => {
  const c = (e.target as HTMLElement).closest<HTMLElement>("[data-color]")?.dataset.color;
  if (!c) return;
  setColor(c);
  toggleColors(false);
});
let customSnapped = false;
colorCustom.addEventListener("input", () => {
  setColor(colorCustom.value, !customSnapped);
  customSnapped = true;
});
colorCustom.addEventListener("change", () => (customSnapped = false));

function toggleColors(open = !colorOpen) {
  colorOpen = open;
  updateChrome();
}

let sliding = false; // one undo step per slider drag, not one per pixel

/** Pen thickness / opacity for new strokes, and for the selected stroke. */
function setPen(size: number, alpha: number) {
  penSize = Math.max(1, Math.min(30, Math.round(size)));
  penAlpha = Math.max(0.1, Math.min(1, alpha));
  const n = notes[picked];
  if (n?.kind === "pen" && (n.size !== penSize || (n.alpha ?? 1) !== penAlpha)) {
    if (!sliding) snapshot();
    sliding = true;
    notes[picked] = { ...n, size: penSize, alpha: penAlpha };
    drawNotes();
  }
  updateChrome();
  renderUI();
}
penSizeInput.addEventListener("input", () => setPen(+penSizeInput.value, penAlpha));
penAlphaInput.addEventListener("input", () => setPen(penSize, +penAlphaInput.value / 100));
for (const el of [penSizeInput, penAlphaInput]) {
  el.addEventListener("change", () => {
    sliding = false;
    el.blur(); // hand the keyboard back to the shortcuts
  });
}

function deletePicked() {
  if (pickedOp && pickedRect()) {
    snapshot();
    const { op, rect } = pickedOp;
    const rects = ops[op].rects.filter((_, j) => j !== rect);
    if (rects.length) ops[op] = { ...ops[op], rects };
    else ops.splice(op, 1);
    pickedOp = null;
    renderImage();
    updateChrome();
    renderUI();
    return;
  }
  if (!notes[picked]) return;
  snapshot();
  notes.splice(picked, 1);
  picked = -1;
  drawNotes();
  updateChrome();
  renderUI();
}

// ---------------------------------------------------------------- loupe

const CELLS = 15; // odd, so one pixel sits in the middle

/** Magnifier + colour readout by the cursor while choosing an area or dragging an edge. */
let loupeOn = false; // per capture; Z toggles

function updateLoupe(p: Point | null) {
  lastPoint = p;
  // Off unless asked for (Z): an always-on magnifier got in the way.
  const show = loupeOn && !!p && !!original && !smart && (!sel || drag?.kind === "new" || drag?.kind === "resize");
  loupe.hidden = !show;
  if (!show || !p || !original) return;
  const s = scale();
  const size = Math.round(120 * s);
  if (loupeCanvas.width !== size) loupeCanvas.width = loupeCanvas.height = size;
  const half = (CELLS - 1) / 2;
  const cell = size / CELLS;
  lctx.imageSmoothingEnabled = false;
  lctx.fillStyle = "#000"; // beyond the screen edge
  lctx.fillRect(0, 0, size, size);
  lctx.drawImage(view, p.x - half, p.y - half, CELLS, CELLS, 0, 0, size, size);
  // The picked pixel: dark + light outline so it shows on any colour.
  lctx.lineWidth = Math.max(1, s);
  lctx.strokeStyle = "#000";
  lctx.strokeRect(half * cell, half * cell, cell, cell);
  lctx.strokeStyle = "#fff";
  lctx.strokeRect(half * cell - lctx.lineWidth, half * cell - lctx.lineWidth, cell + 2 * lctx.lineWidth, cell + 2 * lctx.lineWidth);

  // Colour from the original screenshot: what's really on screen, not our marks.
  const x = Math.min(W - 1, p.x);
  const y = Math.min(H - 1, p.y);
  const i = (y * W + x) * 4;
  const d = original.data;
  hex = "#" + [d[i], d[i + 1], d[i + 2]].map((v) => v.toString(16).padStart(2, "0")).join("").toUpperCase();
  $("loupe-hex").textContent = hex;
  $("loupe-pos").textContent = `${x}, ${y}`;

  // Bottom-right of the cursor; flip to the other side near screen edges.
  const cx = p.x / s;
  const cy = p.y / s;
  const bw = loupe.offsetWidth;
  const bh = loupe.offsetHeight;
  loupe.style.left = `${cx + 20 + bw > window.innerWidth ? cx - 20 - bw : cx + 20}px`;
  loupe.style.top = `${cy + 20 + bh > window.innerHeight ? cy - 20 - bh : cy + 20}px`;
}

async function copyHex() {
  const shown = hex;
  try {
    await invoke("copy_text", { text: shown });
    $("loupe-hex").textContent = "Copied ✓";
    setTimeout(() => lastPoint && updateLoupe(lastPoint), 800);
  } catch (e) {
    console.error("[overlay] copy colour failed", e);
  }
}

// ---------------------------------------------------------------- input

function setTool(t: Tool) {
  tool = t;
  picked = -1;
  pickedOp = null;
  if (t !== "select") closeText(); // the Live Text layer would catch the drag
  updateChrome();
  renderUI();
}

ui.addEventListener("mousedown", (e) => {
  if (e.button !== 0 || busy) return;
  if (colorOpen) toggleColors(false); // a click on the image closes the palette
  const p = toPhys(e);
  if (smart) {
    smartClick(p);
    updateChrome();
    renderUI();
    return;
  }
  if (editing) return void commitNoteInput(); // a click away finishes the text
  const editable = mode === "capture" && sel;
  if (editable && tool === "pen" && inside(p, sel!)) return; // the pen uses pointer events (below)
  const hit = editable && tool === "select" ? noteAt(vctx, notes, p, scale()) : -1;
  // Notes sit above redactions, so a note under the pointer wins.
  // A picked redaction's handles come first: they can sit on the selection's own edge.
  const prevOp = pickedOp;
  const prevRect = pickedRect();
  const opHandle = editable && tool === "select" && prevRect ? handleAt(p, prevRect) : null;
  const opHit = editable && tool === "select" && hit < 0 && inside(p, sel!) && !handleAt(p, sel!) ? opAt(p) : null;
  pickedOp = null;
  if (opHandle && prevOp && prevRect) {
    pickedOp = prevOp;
    snapshot(); // dropped on mouseup if the size didn't change
    drag = { kind: "opmove", origin: p, start: { ...prevRect }, handle: opHandle };
  } else if (editable && isNote(tool) && tool !== "pen" && inside(p, sel!)) {
    picked = -1;
    if (tool === "text") {
      e.preventDefault(); // keep focus in the textarea we're about to show
      return openNoteInput(p);
    }
    snapshot();
    notes.push(newNote(tool, p));
    drawNotes();
    if (tool !== "step") drag = { kind: "note", origin: p };
  } else if (editable && isRedact(tool) && inside(p, sel!)) {
    drag = { kind: "op", origin: p };
    pendingOp = { x: p.x, y: p.y, w: 0, h: 0 };
  } else if (editable && tool === "select" && handleAt(p, sel!)) {
    drag = { kind: "resize", origin: p, start: { ...sel! }, handle: handleAt(p, sel!)! };
  } else if (hit >= 0) {
    picked = hit;
    color = notes[hit].color;
    const n = notes[hit];
    if (n.kind === "pen") [penSize, penAlpha] = [n.size, n.alpha ?? 1];
    snapshot(); // dropped on mouseup if it didn't actually move
    drag = { kind: "notemove", origin: p, start: notes[hit] };
  } else if (opHit) {
    picked = -1;
    pickedOp = opHit;
    snapshot(); // dropped on mouseup if it didn't actually move
    drag = { kind: "opmove", origin: p, start: { ...pickedRect()! } };
  } else if (editable && tool === "select" && inside(p, sel!)) {
    picked = -1;
    drag = { kind: "move", origin: p, start: { ...sel! } };
  } else {
    picked = -1;
    drag = { kind: "new", origin: p, snap: sel ? null : hoverWin };
    hoverWin = null;
    sel = { x: p.x, y: p.y, w: 0, h: 0 };
    closeText();
  }
  updateChrome();
  renderUI();
});

ui.addEventListener("dblclick", (e) => {
  if (mode !== "capture" || tool !== "select") return;
  const i = noteAt(vctx, notes, toPhys(e), scale());
  const n = notes[i];
  if (n?.kind === "text") openNoteInput(n.at, i);
});

window.addEventListener("mousemove", (e) => {
  const p = toPhys(e);
  updateLoupe(p);
  if (!drag && !sel && original) {
    const w = windows.find((r) => inside(p, r)) ?? null;
    if (w !== hoverWin) {
      hoverWin = w;
      renderUI();
    }
  }
  if (!drag) {
    let cursor = "crosshair";
    if (smart) {
      cursor = overWord(p) ? "pointer" : "default";
    } else if (mode === "capture" && sel && tool === "select") {
      const pr = pickedRect();
      const h = (pr && handleAt(p, pr)) || handleAt(p, sel);
      cursor = h ? CURSORS[h] : inside(p, sel) || noteAt(vctx, notes, p, scale()) >= 0 ? "move" : "crosshair";
    } else if (tool === "text" && sel && inside(p, sel)) {
      cursor = "text";
    }
    // The pen keeps Windows' own crosshair (its centre is exactly where ink lands at any display
    // scaling; a custom dot cursor drew off-centre at 150%) plus a size ring drawn in the canvas.
    const hover = tool === "pen" && mode === "capture" && sel && !smart && inside(p, sel) ? p : null;
    if (hover || penHover) {
      penHover = hover;
      renderUI();
    }
    ui.style.cursor = cursor;
    return;
  }
  const dx = p.x - drag.origin.x;
  const dy = p.y - drag.origin.y;
  if (drag.kind === "new") {
    sel = rectFrom(drag.origin, p);
  } else if (drag.kind === "move") {
    const s = drag.start;
    sel = {
      ...s,
      x: Math.max(0, Math.min(W - s.w, s.x + dx)),
      y: Math.max(0, Math.min(H - s.h, s.y + dy)),
    };
  } else if (drag.kind === "resize") {
    const s = drag.start;
    let x0 = s.x, y0 = s.y, x1 = s.x + s.w, y1 = s.y + s.h;
    if (drag.handle.includes("w")) x0 += dx;
    if (drag.handle.includes("e")) x1 += dx;
    if (drag.handle.includes("n")) y0 += dy;
    if (drag.handle.includes("s")) y1 += dy;
    sel = rectFrom({ x: x0, y: y0 }, { x: x1, y: y1 });
  } else if (drag.kind === "op" && sel) {
    pendingOp = clampTo(rectFrom(drag.origin, p), sel);
  } else if (drag.kind === "note" && sel) {
    const n = notes[notes.length - 1];
    const q = { x: Math.max(sel.x, Math.min(sel.x + sel.w, p.x)), y: Math.max(sel.y, Math.min(sel.y + sel.h, p.y)) };
    if (n.kind === "arrow") n.b = q;
    else if (n.kind === "rect" || n.kind === "mark") n.r = rectFrom(drag.origin, q);
    drawNotes();
  } else if (drag.kind === "notemove") {
    notes[picked] = moved(drag.start, dx, dy);
    drawNotes();
  } else if (drag.kind === "opmove" && drag.handle && pickedOp && sel) {
    const s0 = drag.start;
    const min = 4 * scale();
    let x0 = s0.x, y0 = s0.y, x1 = s0.x + s0.w, y1 = s0.y + s0.h;
    // Each dragged edge stops `min` short of the opposite one, so the box never flips or vanishes.
    if (drag.handle.includes("w")) x0 = Math.min(x0 + dx, x1 - min);
    if (drag.handle.includes("e")) x1 = Math.max(x1 + dx, x0 + min);
    if (drag.handle.includes("n")) y0 = Math.min(y0 + dy, y1 - min);
    if (drag.handle.includes("s")) y1 = Math.max(y1 + dy, y0 + min);
    setOpRect(pickedOp, clampTo({ x: x0, y: y0, w: x1 - x0, h: y1 - y0 }, sel));
    renderImageSoon();
  } else if (drag.kind === "opmove" && pickedOp && sel) {
    const r = drag.start; // stays inside the selection, where it was drawn
    const x = Math.max(sel.x, Math.min(sel.x + sel.w - r.w, r.x + dx));
    const y = Math.max(sel.y, Math.min(sel.y + sel.h - r.h, r.y + dy));
    setOpRect(pickedOp, { ...r, x, y });
    renderImageSoon();
  }
  updateChrome();
  renderUI();
});

window.addEventListener("mouseup", () => {
  if (!drag) return;
  const d = drag;
  drag = null;
  if (d.kind === "new") {
    // A click (no real drag) on a window selects that window.
    if (!sel || sel.w < MIN_SIZE || sel.h < MIN_SIZE) sel = d.snap ? { ...d.snap } : null;
    if (sel && mode === "text") {
      loupe.hidden = true;
      renderUI();
      grabText();
      return;
    }
  }
  updateLoupe(lastPoint);
  if (d.kind === "op" && pendingOp && isRedact(tool)) {
    if (pendingOp.w >= MIN_SIZE && pendingOp.h >= MIN_SIZE) addRedaction({ tool, rects: [pendingOp] });
    pendingOp = null;
  }
  if (d.kind === "note" && isTiny(notes[notes.length - 1], scale())) {
    notes.pop(); // an accidental click with a drag tool
    history.pop();
    drawNotes();
  }
  if (d.kind === "notemove" && JSON.stringify(notes[picked]) === JSON.stringify(d.start)) history.pop(); // just a click
  if (d.kind === "opmove") {
    const r = pickedRect();
    if (r && r.x === d.start.x && r.y === d.start.y && r.w === d.start.w && r.h === d.start.h) history.pop(); // just a click
    else renderImage(); // the last move, not just the last frame
  }
  updateChrome();
  renderUI();
});

// ---------------------------------------------------------------- pen
// Pointer events, not mouse ones: they carry pen pressure and every sample the OS took between
// frames (coalesced events), so a fast stroke stays a smooth curve instead of joined straight bits.

const HOLD_MS = 500; // hold still this long to snap (Apple Markup / QuickShape feel)
let holdTimer = 0;
let redrawQueued = false;

/** One redraw per frame, however many pointer samples arrive. */
function redrawSoon() {
  if (redrawQueued) return;
  redrawQueued = true;
  requestAnimationFrame(() => {
    redrawQueued = false;
    drawNotes();
  });
}

const within = (p: Point, r: Rect): Point => ({ x: Math.max(r.x, Math.min(r.x + r.w, p.x)), y: Math.max(r.y, Math.min(r.y + r.h, p.y)) });

function penNote() {
  const n = notes[notes.length - 1];
  return n?.kind === "pen" ? n : null;
}

/** (Re)start the hold timer: if the pointer stays put, the stroke snaps to a clean shape. */
function armHold() {
  clearTimeout(holdTimer);
  holdTimer = window.setTimeout(() => {
    const n = penNote();
    if (drag?.kind !== "pen" || drag.snap || drag.straight || !n) return;
    const shape = recognize(n.pts.map(([x, y]) => ({ x, y })), 40 * scale());
    if (!shape) return;
    n.pts = shape.pts.map((q) => [q.x, q.y, 0.5]);
    n.shape = true;
    drag.snap = { shape, from: drag.at };
    drawNotes();
  }, HOLD_MS);
}

ui.addEventListener("pointerdown", (e) => {
  if (tool !== "pen" || e.button !== 0 || busy || smart || editing || mode !== "capture" || !sel) return;
  const p = toPhys(e);
  if (!inside(p, sel)) return; // outside the selection: mousedown starts a new one, as usual
  e.preventDefault(); // this press is a stroke: no mousedown
  ui.setPointerCapture(e.pointerId);
  snapshot();
  picked = -1;
  const pen = e.pointerType === "pen";
  notes.push({ kind: "pen", color, size: penSize, alpha: penAlpha, pts: [[p.x, p.y, pen ? e.pressure : 0.5]], pen });
  penHover = null; // the ink itself shows where it is now
  drag = { kind: "pen", origin: p, at: p, still: p };
  armHold();
  drawNotes();
  updateChrome();
  renderUI();
});

ui.addEventListener("pointermove", (e) => {
  const n = penNote();
  if (drag?.kind !== "pen" || !sel || !n) return;
  const p = within(toPhys(e), sel);
  drag.at = p;
  if (drag.snap) {
    n.pts = resized(drag.snap.shape, drag.snap.from, p).map((q) => [q.x, q.y, 0.5]);
  } else if (e.shiftKey || drag.straight) {
    // Shift: a straight line from where the stroke began, at 45° steps while Shift is held.
    const [x0, y0] = n.pts[0];
    const end = e.shiftKey ? snapAngle({ x: x0, y: y0 }, p) : p;
    n.pts = [n.pts[0], [end.x, end.y, 0.5]];
    n.shape = drag.straight = true;
  } else {
    const samples = e.getCoalescedEvents?.() ?? [];
    for (const ev of samples.length ? samples : [e]) {
      const q = within(toPhys(ev), sel);
      const [lx, ly] = n.pts[n.pts.length - 1];
      if (Math.hypot(q.x - lx, q.y - ly) >= 1) n.pts.push([q.x, q.y, n.pen ? ev.pressure : 0.5]);
    }
  }
  // Moving on re-arms the hold timer; a few pixels of jitter while holding doesn't.
  if (Math.hypot(p.x - drag.still.x, p.y - drag.still.y) > 4 * scale()) {
    drag.still = p;
    if (!drag.snap) armHold();
  }
  redrawSoon();
});

function endStroke() {
  if (drag?.kind !== "pen") return;
  clearTimeout(holdTimer);
  drag = null;
  drawNotes();
  updateChrome();
  renderUI();
}
ui.addEventListener("pointerup", endStroke);
ui.addEventListener("pointercancel", endStroke);

// Right-click cancels, like Lightshot.
window.addEventListener("contextmenu", (e) => {
  e.preventDefault();
  close();
});

window.addEventListener("keydown", (e) => {
  if (e.target === noteInput) {
    if (e.key === "Escape") commitNoteInput(false);
    else if (e.key === "Enter" && !e.shiftKey) commitNoteInput();
    else return; // typing
    return void e.preventDefault();
  }
  if (e.target === textOut && e.key !== "Escape") return; // let the textarea handle typing, Ctrl+C, etc.
  if (smart) {
    if (e.key === "Escape") cancelSmart();
    if (e.key === "Enter") applySmart();
    return void e.preventDefault();
  }
  if (e.key === "Escape" && !panel.hidden) {
    closeText();
    updateChrome();
    return void e.preventDefault();
  }
  if (e.key === "Escape" && colorOpen) return void toggleColors(false);
  if (e.key === "Escape") return void close();
  if (e.key.toLowerCase() === "z" && !e.ctrlKey && !e.altKey && !e.metaKey && (!sel || drag)) {
    loupeOn = !loupeOn;
    updateLoupe(lastPoint);
    updateChrome();
    return void e.preventDefault();
  }
  if (!loupe.hidden && e.key.toLowerCase() === "c" && !e.ctrlKey && !e.altKey && !e.metaKey) {
    copyHex();
    return void e.preventDefault();
  }
  if (mode !== "capture" || !sel || sel.w < MIN_SIZE) return;

  const k = e.key.toLowerCase();
  const handled = (() => {
    if (e.ctrlKey && k === "z") return undo(), true;
    // With Live Text selected, Ctrl+C copies that text (see the copy listener), not the image.
    if (e.ctrlKey && k === "c") return getSelection()?.isCollapsed === false ? false : (finish("copy"), true);
    if (e.ctrlKey && k === "s") return finish("saveas"), true;
    if (e.ctrlKey || e.altKey || e.metaKey) return false;
    if (k === "enter") return finish("save"), true;
    if ((k === "delete" || k === "backspace") && (notes[picked] || pickedRect())) return deletePicked(), true;
    if (k === "c") return cycleColor(), true;
    if (k === "f") return void togglePinning(), true;
    if (k === "l") return scrollCapture(), true;
    const tools: Record<string, Tool> = {
      v: "select", b: "blur", p: "pixelate", r: "box",
      a: "arrow", o: "rect", h: "mark", w: "text", n: "step", d: "pen",
    };
    if (tools[k]) return setTool(tools[k]), true;
    if (k === "[" || k === "]") return setPen(penSize + (k === "]" ? 1 : -1) * (penSize >= 10 ? 2 : 1), penAlpha), true;
    if (k === "t") return extractText(), true;
    if (k === "s") return startSmart(), true;
    return false;
  })();
  if (handled) e.preventDefault();
});

toolbar.addEventListener("click", (e) => {
  const btn = (e.target as HTMLElement).closest("button");
  if (!btn) return;
  if (btn.dataset.tool) return setTool(btn.dataset.tool as Tool);
  switch (btn.dataset.action) {
    case "undo": return undo();
    case "ocr": return void extractText();
    case "smart": return void startSmart();
    case "color": return toggleColors();
    case "pinning": return void togglePinning();
    case "scroll": return void scrollCapture();
    case "copy":
    case "save":
    case "saveas": return void finish(btn.dataset.action);
    case "close": return void close();
  }
});
toolbar.addEventListener("pointerover", (e) => {
  const btn = (e.target as HTMLElement).closest<HTMLElement>("[data-name]");
  if (!btn) return;
  clearTimeout(tipTimer);
  const warm = !tip.hidden || performance.now() - tipHiddenAt < 600;
  tipTimer = window.setTimeout(() => showTip(btn), warm ? 0 : 350);
});
toolbar.addEventListener("pointerleave", hideTip);
toolbar.querySelectorAll<HTMLElement>("[data-name]").forEach((b) => b.setAttribute("aria-label", b.dataset.name!));

// Keep toolbar clicks from starting a new selection underneath.
toolbar.addEventListener("mousedown", (e) => {
  e.stopPropagation();
  hideTip();
});
panel.addEventListener("mousedown", (e) => e.stopPropagation());
smartbar.addEventListener("mousedown", (e) => e.stopPropagation());
noteInput.addEventListener("mousedown", (e) => e.stopPropagation());
noteInput.addEventListener("blur", () => commitNoteInput()); // e.g. clicking Copy while typing
$("smart-cancel").addEventListener("click", cancelSmart);
smartApply.addEventListener("click", applySmart);

$("text-close").addEventListener("click", () => {
  closeText();
  updateChrome();
});
$("text-copy").addEventListener("click", () => copyValue(textOut.value));
$("text-ai").addEventListener("click", aiReread);

// ---------------------------------------------------------------- wiring

const win = getCurrentWebviewWindow();
win.listen<OpenPayload>("overlay:open", (e) => open(e.payload));
win.listen("overlay:reset", reset);
win.listen<[number, number, number, number][]>("overlay:windows", (e) => {
  windows = e.payload.map(([x, y, w, h]) => ({ x, y, w, h }));
});
document.addEventListener("mouseleave", () => updateLoupe(null));
// In case the open event fired before this page finished loading.
invoke<OpenPayload | null>("overlay_pending").then((p) => p && open(p));
