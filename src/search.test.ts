// Run: npm test
import { test } from "node:test";
import assert from "node:assert/strict";
import { matches, snippet } from "./search.ts";

const ocr = "INVOICE #2026-0042\nBilled to: Acme Ltd\nTotal: $1,249.99";

test("all words must match, any order and case", () => {
  assert.ok(matches(ocr, "invoice"));
  assert.ok(matches(ocr, "acme   INVOICE"));
  assert.ok(!matches(ocr, "invoice refund"));
  assert.ok(matches(ocr, ""), "empty query shows everything");
});

test("snippet marks the first word with context", () => {
  assert.deepEqual(snippet(ocr, "acme", 8), ["…led to: ", "Acme", " Ltd Tot…"], "8 chars each side, newline flattened");
  assert.deepEqual(snippet("short", "short"), ["", "short", ""]);
  assert.equal(snippet(ocr, "nope"), null);
});
