// Run: npm test
import { test } from "node:test";
import assert from "node:assert/strict";
import { recognize, resized, snapAngle, strokePath } from "./ink.ts";

type P = { x: number; y: number };
// Repeatable hand wobble: up to ±amp px.
let seed = 7;
const wobble = (amp: number) => ((seed = (seed * 16807) % 2147483647) / 2147483647 - 0.5) * 2 * amp;
const along = (a: P, b: P, n: number, amp: number): P[] =>
  Array.from({ length: n }, (_, i) => ({ x: a.x + ((b.x - a.x) * i) / n + wobble(amp), y: a.y + ((b.y - a.y) * i) / n + wobble(amp) }));
const MIN = 30;

test("a wobbly near-level stroke snaps to a level line", () => {
  const s = recognize(along({ x: 10, y: 100 }, { x: 310, y: 112 }, 60, 2), MIN);
  assert.equal(s?.kind, "line");
  assert.ok(Math.abs(s!.pts[1].y - s!.pts[0].y) < 0.001, "2° off level gets straightened");
  const steep = recognize(along({ x: 0, y: 0 }, { x: 200, y: 120 }, 60, 2), MIN);
  assert.equal(steep?.kind, "line");
  assert.ok(Math.abs(steep!.pts[1].y - 120) < 5, "a clearly sloped line keeps its slope");
});

test("a rough box snaps to a box, a rough circle to an ellipse", () => {
  const c = [{ x: 0, y: 0 }, { x: 300, y: 0 }, { x: 300, y: 150 }, { x: 0, y: 150 }];
  const box = [...along(c[0], c[1], 40, 3), ...along(c[1], c[2], 20, 3), ...along(c[2], c[3], 40, 3), ...along(c[3], { x: 4, y: 6 }, 20, 3)];
  const r = recognize(box, MIN);
  assert.equal(r?.kind, "rect");
  assert.equal(r!.pts.length, 5, "closed: back to the first corner");

  const ring = Array.from({ length: 90 }, (_, i) => {
    const t = (i / 90) * Math.PI * 2 * 0.97; // ends a little short of where it started
    return { x: 200 + 120 * Math.cos(t) + wobble(3), y: 200 + 80 * Math.sin(t) + wobble(3) };
  });
  const e = recognize(ring, MIN);
  assert.equal(e?.kind, "ellipse");
  const xs = e!.pts.map((p) => p.x);
  assert.ok(Math.abs(Math.max(...xs) - Math.min(...xs) - 240) < 10, "keeps its width");
});

test("open curves, scribbles and tiny strokes stay as ink", () => {
  const arc = Array.from({ length: 50 }, (_, i) => ({ x: 100 * Math.cos((i / 50) * Math.PI), y: 100 * Math.sin((i / 50) * Math.PI) }));
  assert.equal(recognize(arc, MIN), null, "a U shape isn't closed");
  const scribble = Array.from({ length: 120 }, (_, i) => ({ x: 100 + 60 * Math.sin(i * 0.9) + wobble(30), y: 100 + 60 * Math.cos(i * 1.7) + wobble(30) }));
  assert.equal(recognize(scribble, MIN), null);
  assert.equal(recognize(along({ x: 0, y: 0 }, { x: 10, y: 0 }, 5, 0), MIN), null, "too short");
});

test("Shift snaps to 45°, and a snapped shape resizes as you drag on", () => {
  const p = snapAngle({ x: 0, y: 0 }, { x: 100, y: 90 });
  assert.ok(Math.abs(p.x - p.y) < 0.001, "100,90 is closest to the 45° diagonal");
  const box = { kind: "rect" as const, pts: [{ x: 0, y: 0 }, { x: 100, y: 0 }, { x: 100, y: 100 }, { x: 0, y: 100 }, { x: 0, y: 0 }] };
  const big = resized(box, { x: 100, y: 100 }, { x: 150, y: 150 });
  assert.deepEqual(big[2], { x: 150, y: 150 }, "corner follows the pointer");
  assert.deepEqual(big[0], { x: -50, y: -50 }, "scaled about the centre");
});

test("a stroke and a single click both give an outline", () => {
  assert.match(strokePath([[0, 0, 0.5], [50, 10, 0.5], [100, 0, 0.5]], 8, false), /^M.*Z$/);
  assert.match(strokePath([[5, 5, 0.5]], 8, false), /^M.*Z$/, "a dot");
});
