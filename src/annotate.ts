// Annotations: pen strokes, arrows, boxes, highlighter, text and numbered steps, drawn over the
// screenshot (always above redactions). Geometry is physical pixels like the rest of the
// overlay; `s` is physical px per CSS px, so strokes look the same at any display scaling.
// Pure apart from the canvas it's given, so hit-testing is unit-tested (annotate.test.ts).

import { strokePath, type InkPoint } from "./ink.ts";

export type Point = { x: number; y: number };
export type Rect = { x: number; y: number; w: number; h: number };
export type Note =
  | { kind: "arrow"; color: string; a: Point; b: Point }
  | { kind: "rect" | "mark"; color: string; r: Rect }
  | { kind: "text"; color: string; at: Point; text: string }
  | { kind: "step"; color: string; at: Point }
  // size: CSS px. alpha: opacity (1 if unset). pen: real stylus pressure. shape: snapped to a
  // clean line/box/ellipse.
  | { kind: "pen"; color: string; size: number; alpha?: number; pts: InkPoint[]; pen: boolean; shape?: boolean };
export type NoteKind = Note["kind"];

export const COLORS = ["#ef4444", "#facc15", "#22c55e", "#3b82f6", "#18181b"];
export const FONT_PX = 18;
const LINE = 1.25;
const STEP_R = 13;
export const textFont = (s: number) => `600 ${FONT_PX * s}px "Segoe UI", sans-serif`;

/** Draws one note. `step` is its number among step badges (they renumber on delete). */
export function drawNote(c: CanvasRenderingContext2D, n: Note, s: number, step: number) {
  c.save();
  c.strokeStyle = c.fillStyle = n.color;
  c.lineWidth = 3 * s;
  c.lineCap = c.lineJoin = "round";
  // A soft shadow keeps marks readable on both light and dark screenshots.
  c.shadowColor = "rgba(0, 0, 0, 0.35)";
  c.shadowBlur = 3 * s;
  switch (n.kind) {
    case "rect":
      c.strokeRect(n.r.x, n.r.y, n.r.w, n.r.h);
      break;
    case "mark":
      c.shadowColor = "transparent";
      c.globalCompositeOperation = "multiply"; // like a real highlighter: text stays crisp
      c.globalAlpha = 0.45;
      c.fillRect(n.r.x, n.r.y, n.r.w, n.r.h);
      break;
    case "arrow": {
      const { a, b } = n;
      const ang = Math.atan2(b.y - a.y, b.x - a.x);
      const head = Math.min(18 * s, Math.hypot(b.x - a.x, b.y - a.y) * 0.6);
      const back = (d: number, da: number) => ({ x: b.x - d * Math.cos(ang + da), y: b.y - d * Math.sin(ang + da) });
      const neck = back(head * 0.7, 0);
      c.beginPath();
      c.moveTo(a.x, a.y);
      c.lineTo(neck.x, neck.y);
      c.stroke();
      const l = back(head, 0.45);
      const r = back(head, -0.45);
      c.beginPath();
      c.moveTo(b.x, b.y);
      c.lineTo(l.x, l.y);
      c.lineTo(r.x, r.y);
      c.closePath();
      c.fill();
      break;
    }
    case "step":
      c.beginPath();
      c.arc(n.at.x, n.at.y, STEP_R * s, 0, Math.PI * 2);
      c.fill();
      c.shadowColor = "transparent";
      c.fillStyle = n.color === COLORS[1] ? "#18181b" : "#fff"; // dark number on yellow
      c.font = `700 ${14 * s}px "Segoe UI", sans-serif`;
      c.textAlign = "center";
      c.textBaseline = "middle";
      c.fillText(String(step), n.at.x, n.at.y + s);
      break;
    case "pen":
      if ((n.alpha ?? 1) < 1) {
        c.globalAlpha = n.alpha!;
        c.shadowColor = "transparent"; // a shadow under see-through ink just looks muddy
      }
      if (n.shape) {
        c.lineWidth = n.size * s * 0.8; // matches mouse ink, which thins a little with speed
        c.beginPath();
        n.pts.forEach(([x, y], i) => (i ? c.lineTo(x, y) : c.moveTo(x, y)));
        c.stroke();
      } else {
        c.fill(new Path2D(strokePath(n.pts, n.size * s, n.pen)));
      }
      break;
    case "text":
      c.font = textFont(s);
      c.textBaseline = "top";
      // Same line spacing as the CSS textarea it was typed in (half-leading above each line).
      n.text.split("\n").forEach((line, i) => c.fillText(line, n.at.x, n.at.y + (i * LINE + (LINE - 1) / 2) * FONT_PX * s));
      break;
  }
  c.restore();
}

/** Bounding box, for hit-testing and the selection outline. */
export function noteBox(c: CanvasRenderingContext2D, n: Note, s: number): Rect {
  switch (n.kind) {
    case "arrow": {
      const x = Math.min(n.a.x, n.b.x);
      const y = Math.min(n.a.y, n.b.y);
      return { x, y, w: Math.abs(n.a.x - n.b.x), h: Math.abs(n.a.y - n.b.y) };
    }
    case "rect":
    case "mark":
      return n.r;
    case "step": {
      const r = STEP_R * s;
      return { x: n.at.x - r, y: n.at.y - r, w: 2 * r, h: 2 * r };
    }
    case "pen": {
      const r = (n.size * s) / 2;
      const xs = n.pts.map((p) => p[0]);
      const ys = n.pts.map((p) => p[1]);
      const x = Math.min(...xs) - r;
      const y = Math.min(...ys) - r;
      return { x, y, w: Math.max(...xs) + r - x, h: Math.max(...ys) + r - y };
    }
    case "text": {
      c.font = textFont(s);
      const lines = n.text.split("\n");
      const w = Math.max(...lines.map((l) => c.measureText(l).width));
      return { x: n.at.x, y: n.at.y, w, h: lines.length * FONT_PX * LINE * s };
    }
  }
}

function distToSegment(p: Point, a: Point, b: Point) {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const t = dx || dy ? Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / (dx * dx + dy * dy))) : 0;
  return Math.hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy));
}

/** Index of the topmost note under `p`, or -1. Box outlines only grab near their edge, so
 * clicks inside a box still reach whatever is under it. */
export function noteAt(c: CanvasRenderingContext2D, notes: Note[], p: Point, s: number): number {
  const t = 8 * s;
  for (let i = notes.length - 1; i >= 0; i--) {
    const n = notes[i];
    if (n.kind === "arrow") {
      if (distToSegment(p, n.a, n.b) <= t) return i;
      continue;
    }
    if (n.kind === "pen") {
      // Near the ink itself, not anywhere in its bounding box (a circle shouldn't swallow clicks).
      const at = (k: number) => ({ x: n.pts[k][0], y: n.pts[k][1] });
      const reach = t + (n.size * s) / 2;
      for (let k = 0; k < n.pts.length; k++) {
        if (distToSegment(p, at(Math.max(0, k - 1)), at(k)) <= reach) return i;
      }
      continue;
    }
    const r = noteBox(c, n, s);
    const inOuter = p.x >= r.x - t && p.x <= r.x + r.w + t && p.y >= r.y - t && p.y <= r.y + r.h + t;
    const inInner = p.x > r.x + t && p.x < r.x + r.w - t && p.y > r.y + t && p.y < r.y + r.h - t;
    if (inOuter && !(n.kind === "rect" && inInner)) return i;
  }
  return -1;
}

export function moved(n: Note, dx: number, dy: number): Note {
  const t = (p: Point) => ({ x: p.x + dx, y: p.y + dy });
  switch (n.kind) {
    case "arrow":
      return { ...n, a: t(n.a), b: t(n.b) };
    case "rect":
    case "mark":
      return { ...n, r: { ...n.r, x: n.r.x + dx, y: n.r.y + dy } };
    case "pen":
      return { ...n, pts: n.pts.map(([x, y, p]) => [x + dx, y + dy, p]) };
    default:
      return { ...n, at: t(n.at) };
  }
}

/** Too small to keep (an accidental click with a drag tool). */
export function isTiny(n: Note, s: number) {
  if (n.kind === "arrow") return Math.hypot(n.b.x - n.a.x, n.b.y - n.a.y) < 6 * s;
  if (n.kind === "rect" || n.kind === "mark") return n.r.w < 4 * s || n.r.h < 4 * s;
  return false;
}
