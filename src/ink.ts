// Pen helpers: the filled outline of a freehand stroke (perfect-freehand, the algorithm behind
// tldraw and Excalidraw), and hold-to-snap shape recognition (draw a rough line, circle or box,
// hold still, and it becomes a clean one, like Apple Markup / Procreate QuickShape).
// Pure, so recognition is unit-tested (ink.test.ts).

import { getStroke } from "perfect-freehand";

type Point = { x: number; y: number };
/** [x, y, pressure] in physical pixels. */
export type InkPoint = [number, number, number];
export type Shape = { kind: "line" | "rect" | "ellipse"; pts: Point[] };

/** Filled outline of a freehand stroke as an SVG path. `pen` = real stylus pressure; a mouse
 * gets pressure from speed instead (fast = thinner), which is what makes it feel like ink. */
export function strokePath(pts: InkPoint[], size: number, pen: boolean): string {
  const outline = getStroke(pts, {
    size,
    thinning: 0.5,
    smoothing: 0.5,
    streamline: 0.5,
    simulatePressure: !pen,
    last: true,
  });
  return svgPath(outline);
}

/** Outline points as smooth quadratic curves (from the perfect-freehand README). */
function svgPath(points: number[][]): string {
  if (points.length < 4) return "";
  const mid = (a: number[], b: number[]) => `${((a[0] + b[0]) / 2).toFixed(2)},${((a[1] + b[1]) / 2).toFixed(2)}`;
  const [a, b, c] = points;
  let d = `M${a[0].toFixed(2)},${a[1].toFixed(2)} Q${b[0].toFixed(2)},${b[1].toFixed(2)} ${mid(b, c)} T`;
  for (let i = 2; i < points.length - 1; i++) d += `${mid(points[i], points[i + 1])} `;
  return d + "Z";
}

const dist = (a: Point, b: Point) => Math.hypot(a.x - b.x, a.y - b.y);

/** End point moved onto the nearest multiple of `step` radians from `a` (Shift = 45°). */
export function snapAngle(a: Point, b: Point, step = Math.PI / 4): Point {
  const r = dist(a, b);
  const ang = Math.round(Math.atan2(b.y - a.y, b.x - a.x) / step) * step;
  return { x: a.x + r * Math.cos(ang), y: a.y + r * Math.sin(ang) };
}

/** A near-level or near-upright line gets straightened (an underline that's 3° off looks sloppy). */
function level(a: Point, b: Point): Point {
  const ang = Math.atan2(b.y - a.y, b.x - a.x);
  const off = Math.abs(ang - Math.round(ang / (Math.PI / 2)) * (Math.PI / 2));
  return off < (5 * Math.PI) / 180 ? snapAngle(a, b, Math.PI / 2) : b;
}

/** Clean shape for a rough stroke, or null if it doesn't look like one. `min` = shortest stroke
 * worth snapping (physical px).
 * - Line: the ends are nearly as far apart as the path is long.
 * - Closed (ends meet): box if the path passes close to all 4 corners of its bounding box, an
 *   ellipse if not (a circle misses the corners by ~20% of its size); a scribble fits neither. */
export function recognize(pts: Point[], min: number): Shape | null {
  if (pts.length < 2) return null;
  // Length over points at least min/8 apart: pixel-level hand jitter would otherwise add 20-30%
  // to a straight stroke's length and stop it reading as a line.
  let len = 0;
  let last = pts[0];
  for (const p of pts) {
    const d = dist(last, p);
    if (d >= min / 8 || p === pts[pts.length - 1]) {
      len += d;
      last = p;
    }
  }
  const a = pts[0];
  const b = pts[pts.length - 1];
  if (len < min) return null;
  if (dist(a, b) / len > 0.92) return { kind: "line", pts: [a, level(a, b)] };

  const xs = pts.map((p) => p.x);
  const ys = pts.map((p) => p.y);
  const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
  const w = x1 - x0;
  const h = y1 - y0;
  const small = Math.min(w, h);
  if (small < min / 6 || dist(a, b) > 0.25 * Math.max(w, h)) return null; // open curve or a flat squiggle

  const corners = [{ x: x0, y: y0 }, { x: x1, y: y0 }, { x: x1, y: y1 }, { x: x0, y: y1 }];
  const cornerMiss = corners.reduce((sum, c) => sum + Math.min(...pts.map((p) => dist(p, c))), 0) / 4 / small;
  if (cornerMiss < 0.12) {
    // Every point should sit near an edge of the box.
    const off = pts.reduce((sum, p) => sum + Math.min(p.x - x0, x1 - p.x, p.y - y0, y1 - p.y), 0) / pts.length / small;
    return off < 0.08 ? { kind: "rect", pts: [...corners, corners[0]] } : null;
  }
  const cx = (x0 + x1) / 2;
  const cy = (y0 + y1) / 2;
  const off = pts.reduce((sum, p) => sum + Math.abs(Math.hypot((p.x - cx) / (w / 2), (p.y - cy) / (h / 2)) - 1), 0) / pts.length;
  if (off > 0.15) return null;
  const ring = Array.from({ length: 73 }, (_, i) => {
    const t = (i / 72) * Math.PI * 2;
    return { x: cx + (w / 2) * Math.cos(t), y: cy + (h / 2) * Math.sin(t) };
  });
  return { kind: "ellipse", pts: ring };
}

/** A snapped shape, resized by dragging on after the snap: a line's end follows the pointer;
 * a box or ellipse scales about its centre. */
export function resized(shape: Shape, from: Point, to: Point): Point[] {
  if (shape.kind === "line") return [shape.pts[0], level(shape.pts[0], to)];
  const xs = shape.pts.map((p) => p.x);
  const ys = shape.pts.map((p) => p.y);
  const c = { x: (Math.min(...xs) + Math.max(...xs)) / 2, y: (Math.min(...ys) + Math.max(...ys)) / 2 };
  const k = dist(c, to) / Math.max(1, dist(c, from));
  return shape.pts.map((p) => ({ x: c.x + (p.x - c.x) * k, y: c.y + (p.y - c.y) * k }));
}
