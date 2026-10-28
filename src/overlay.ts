// Capture overlay: one of these runs per monitor, kept hidden until a hotkey fires.
//
// All geometry is in PHYSICAL pixels (the screenshot's own pixels), so selection and
// redaction stay sharp and exact at any Windows display scaling. Mouse positions are
// converted from CSS pixels with `scale()`.

import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

type Mode = "capture" | "text";
type Tool = "select" | "blur" | "pixelate" | "box";
type RedactTool = Exclude<Tool, "select">;
type Rect = { x: number; y: number; w: number; h: number };
type Point = { x: number; y: number };
type Handle = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";
type OpenPayload = { mode: Mode; width: number; height: number };
type Op = { tool: RedactTool; rect: Rect };
type Drag =
  | { kind: "new"; origin: Point }
  | { kind: "move"; origin: Point; start: Rect }
  | { kind: "resize"; origin: Point; start: Rect; handle: Handle }
  | { kind: "op"; origin: Point };

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
const tmp = document.createElement("canvas");
const tctx = tmp.getContext("2d")!;

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

/** Burn one redaction into the view canvas. Pixels underneath are destroyed. */
function applyOp({ tool, rect: { x, y, w, h } }: Op) {
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

function renderImage() {
  if (!original) return;
  vctx.putImageData(original, 0, 0);
  ops.forEach(applyOp);
}

function undo() {
  if (ops.pop()) renderImage();
  updateChrome();
}

// ---------------------------------------------------------------- drawing UI

function renderUI() {
  uctx.clearRect(0, 0, W, H);
  uctx.fillStyle = "rgba(0, 0, 0, 0.45)";
  uctx.fillRect(0, 0, W, H);
  if (!sel || sel.w === 0 || sel.h === 0) return;

  const s = scale();
  uctx.clearRect(sel.x, sel.y, sel.w, sel.h);
  uctx.strokeStyle = ACCENT;
  uctx.lineWidth = Math.max(1, Math.round(s));
  uctx.strokeRect(sel.x + 0.5, sel.y + 0.5, sel.w - 1, sel.h - 1);

  if (mode === "capture" && drag?.kind !== "new") {
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
}

/** Position the DOM bits (hint, size label, toolbar) around the selection. */
function updateChrome() {
  const s = scale();
  const has = !!sel && sel.w >= MIN_SIZE && sel.h >= MIN_SIZE;

  hint.hidden = !!sel;
  hint.textContent =
    mode === "text"
      ? "Drag over text to copy it  ·  Esc to cancel"
      : "Drag to select an area  ·  Esc to cancel";

  sizeLabel.hidden = !has;
  if (has && sel) {
    sizeLabel.textContent = `${sel.w} × ${sel.h}`;
    const top = sel.y / s - 24;
    sizeLabel.style.left = `${sel.x / s}px`;
    sizeLabel.style.top = `${top < 4 ? sel.y / s + 4 : top}px`;
  }

  const showBar = has && mode === "capture" && !drag;
  toolbar.hidden = !showBar;
  if (showBar && sel) {
    toolbar.querySelectorAll<HTMLButtonElement>("[data-tool]").forEach((b) =>
      b.classList.toggle("active", b.dataset.tool === tool),
    );
    toolbar.querySelector<HTMLButtonElement>('[data-action="undo"]')!.disabled = ops.length === 0;
    placeNear(toolbar, sel);
  }
  if (!panel.hidden && sel) placePanel();
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
    view.width = ui.width = W;
    view.height = ui.height = H;
    original = new ImageData(new Uint8ClampedArray(await receiveFrame()), W, H);
    vctx.putImageData(original, 0, 0);
    sel = null;
    ops = [];
    tool = "select";
    renderUI();
    updateChrome();
    const ms = await invoke<number>("overlay_shown");
    console.log(`[overlay] visible ${ms} ms after hotkey`);
  } catch (e) {
    console.error("[overlay] open failed", e);
    opened = false;
  }
}

function reset() {
  opened = false;
  busy = false;
  original = null;
  sel = null;
  ops = [];
  drag = null;
  pendingOp = null;
  toolbar.hidden = panel.hidden = sizeLabel.hidden = hint.hidden = true;
  // Shrink canvases so a hidden overlay doesn't hold a full screenshot in memory.
  view.width = view.height = ui.width = ui.height = 1;
  W = H = 0;
}

const close = () => invoke("overlay_close");

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
  try {
    await invoke("finish", bytes, { headers: { ...headers, "x-action": action } });
  } catch (e) {
    console.error("[overlay] finish failed", e);
    busy = false;
  }
}

async function grabText() {
  if (!sel || busy) return;
  busy = true;
  const { bytes, headers } = cropSelection();
  try {
    await invoke("grab_text", bytes, { headers });
  } catch (e) {
    console.error("[overlay] grab_text failed", e);
    busy = false;
  }
}

async function extractText() {
  if (!sel || busy) return;
  panel.hidden = false;
  textStatus.textContent = "Reading text…";
  textOut.value = "";
  placePanel();
  const { bytes, headers } = cropSelection();
  try {
    const text = await invoke<string>("ocr_image", bytes, { headers });
    textOut.value = text;
    const words = text.split(/\s+/).filter(Boolean).length;
    textStatus.textContent = text.trim() ? `${words} words found — edit, then copy` : "No text found";
    textOut.focus();
  } catch (e) {
    textStatus.textContent = String(e);
  }
}

async function copyText() {
  try {
    await invoke("copy_text", { text: textOut.value });
    textStatus.textContent = "Copied ✓";
  } catch (e) {
    textStatus.textContent = String(e);
  }
}

// ---------------------------------------------------------------- input

function setTool(t: Tool) {
  tool = t;
  updateChrome();
}

ui.addEventListener("mousedown", (e) => {
  if (e.button !== 0 || busy) return;
  const p = toPhys(e);
  const editable = mode === "capture" && sel;
  if (editable && tool !== "select" && inside(p, sel!)) {
    drag = { kind: "op", origin: p };
    pendingOp = { x: p.x, y: p.y, w: 0, h: 0 };
  } else if (editable && tool === "select" && handleAt(p, sel!)) {
    drag = { kind: "resize", origin: p, start: { ...sel! }, handle: handleAt(p, sel!)! };
  } else if (editable && tool === "select" && inside(p, sel!)) {
    drag = { kind: "move", origin: p, start: { ...sel! } };
  } else {
    drag = { kind: "new", origin: p };
    sel = { x: p.x, y: p.y, w: 0, h: 0 };
    panel.hidden = true;
  }
  updateChrome();
  renderUI();
});

window.addEventListener("mousemove", (e) => {
  const p = toPhys(e);
  if (!drag) {
    let cursor = "crosshair";
    if (mode === "capture" && sel && tool === "select") {
      const h = handleAt(p, sel);
      cursor = h ? CURSORS[h] : inside(p, sel) ? "move" : "crosshair";
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
  }
  updateChrome();
  renderUI();
});

window.addEventListener("mouseup", () => {
  if (!drag) return;
  const d = drag;
  drag = null;
  if (d.kind === "new") {
    if (!sel || sel.w < MIN_SIZE || sel.h < MIN_SIZE) {
      sel = null;
    } else if (mode === "text") {
      renderUI();
      grabText();
      return;
    }
  }
  if (d.kind === "op" && pendingOp && tool !== "select") {
    if (pendingOp.w >= MIN_SIZE && pendingOp.h >= MIN_SIZE) {
      const op: Op = { tool, rect: pendingOp };
      ops.push(op);
      applyOp(op);
    }
    pendingOp = null;
  }
  updateChrome();
  renderUI();
});

// Right-click cancels, like Lightshot.
window.addEventListener("contextmenu", (e) => {
  e.preventDefault();
  close();
});

window.addEventListener("keydown", (e) => {
  if (e.target === textOut) {
    if (e.key === "Escape") {
      panel.hidden = true;
      e.preventDefault();
    }
    return; // let the textarea handle typing, Ctrl+C, etc.
  }
  if (e.key === "Escape") return void close();
  if (mode !== "capture" || !sel || sel.w < MIN_SIZE) return;

  const k = e.key.toLowerCase();
  const handled = (() => {
    if (e.ctrlKey && k === "z") return undo(), true;
    if (e.ctrlKey && k === "c") return finish("copy"), true;
    if (e.ctrlKey && k === "s") return finish("saveas"), true;
    if (e.ctrlKey || e.altKey || e.metaKey) return false;
    const tools: Record<string, Tool> = { v: "select", b: "blur", p: "pixelate", r: "box" };
    if (tools[k]) return setTool(tools[k]), true;
    if (k === "t") return extractText(), true;
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
    case "copy":
    case "save":
    case "both":
    case "saveas": return void finish(btn.dataset.action);
    case "close": return void close();
  }
});
// Keep toolbar clicks from starting a new selection underneath.
toolbar.addEventListener("mousedown", (e) => e.stopPropagation());
panel.addEventListener("mousedown", (e) => e.stopPropagation());

$("text-close").addEventListener("click", () => (panel.hidden = true));
$("text-copy").addEventListener("click", copyText);

// ---------------------------------------------------------------- wiring

const win = getCurrentWebviewWindow();
win.listen<OpenPayload>("overlay:open", (e) => open(e.payload));
win.listen("overlay:reset", reset);
// In case the open event fired before this page finished loading.
invoke<OpenPayload | null>("overlay_pending").then((p) => p && open(p));
