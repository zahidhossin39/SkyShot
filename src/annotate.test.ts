// Run: npm test
import { test } from "node:test";
import assert from "node:assert/strict";
import { isTiny, moved, noteAt, noteBox, type Note } from "./annotate.ts";

// Text width = 10px per character, enough to test text boxes without a real canvas.
const c = { font: "", measureText: (t: string) => ({ width: t.length * 10 }) } as unknown as CanvasRenderingContext2D;
const red = "#ef4444";

test("hit-testing picks the topmost note, arrows by distance, boxes by edge", () => {
  const notes: Note[] = [
    { kind: "rect", color: red, r: { x: 0, y: 0, w: 200, h: 200 } },
    { kind: "arrow", color: red, a: { x: 0, y: 300 }, b: { x: 100, y: 300 } },
    { kind: "step", color: red, at: { x: 150, y: 150 } },
    { kind: "text", color: red, at: { x: 400, y: 0 }, text: "hi\nlonger" },
  ];
  assert.equal(noteAt(c, notes, { x: 1, y: 100 }, 1), 0, "on the box edge");
  assert.equal(noteAt(c, notes, { x: 100, y: 100 }, 1), -1, "inside the box, away from its edge");
  assert.equal(noteAt(c, notes, { x: 50, y: 305 }, 1), 1, "near the arrow line");
  assert.equal(noteAt(c, notes, { x: 50, y: 330 }, 1), -1, "far from the arrow");
  assert.equal(noteAt(c, notes, { x: 150, y: 150 }, 1), 2, "step inside the box wins (on top)");
  assert.equal(noteAt(c, notes, { x: 455, y: 30 }, 1), 3, "second, longer text line");
});

test("text box grows with lines and longest line", () => {
  assert.deepEqual(noteBox(c, { kind: "text", color: red, at: { x: 5, y: 5 }, text: "ab\nabcd" }, 2), { x: 5, y: 5, w: 40, h: 90 });
});

test("moving and tiny checks", () => {
  const a: Note = { kind: "arrow", color: red, a: { x: 0, y: 0 }, b: { x: 10, y: 0 } };
  assert.deepEqual(moved(a, 5, 7), { ...a, a: { x: 5, y: 7 }, b: { x: 15, y: 7 } });
  assert.equal(isTiny(a, 1), false);
  assert.equal(isTiny(a, 2), true, "10px is too short at 200% scaling");
  assert.equal(isTiny({ kind: "mark", color: red, r: { x: 0, y: 0, w: 50, h: 2 } }, 1), true);
});
