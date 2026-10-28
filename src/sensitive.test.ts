// Run: npm test
import { test } from "node:test";
import assert from "node:assert/strict";
import { findSensitive, type Word } from "./sensitive.ts";

/** One OCR line from a sentence: each word 10px wide, laid out left to right. */
const line = (s: string): Word[] => s.split(" ").map((text, i) => ({ x: i * 10, y: 0, w: 8, h: 10, text }));
const found = (s: string) => findSensitive([line(s)]).map((f) => [f.kind, f.words.map((w) => w.text).join(" ")]);

test("finds each kind and spans multi-word numbers", () => {
  assert.deepEqual(found("mail me at zaid.h+test@example.co.uk today"), [["email", "zaid.h+test@example.co.uk"]]);
  assert.deepEqual(found("server 192.168.1.20 is down"), [["ip", "192.168.1.20"]]);
  assert.deepEqual(found("card 4242 4242 4242 4242 exp 12/28"), [["card", "4242 4242 4242 4242"]]);
  assert.deepEqual(found("call +1 (555) 123-4567 now"), [["phone", "+1 (555) 123-4567"]]);
  assert.deepEqual(found("OPENAI_KEY=sk-proj-4fJ9xQ2LmZ8wR1tY6uP3"), [["key", "OPENAI_KEY=sk-proj-4fJ9xQ2LmZ8wR1tY6uP3"]]);
  assert.deepEqual(found("aws AKIAIOSFODNN7EXAMPLE"), [["key", "AKIAIOSFODNN7EXAMPLE"]]);
  assert.deepEqual(found("dark user@example.com called +1 555 0100"), [["email", "user@example.com"], ["phone", "+1 555 0100"]]);
});

// Cases from real Windows OCR output on the spike samples.
test("real OCR quirks", () => {
  assert.deepEqual(found("sk-proj -4fG7hK2mN9pQ3rT6vX8zA1bC5dEe"), [["key", "sk-proj -4fG7hK2mN9pQ3rT6vX8zA1bC5dEe"]]);
  assert.deepEqual(found("https://github.com/tauri-apps/tauri/issues/9821"), []);
  assert.deepEqual(found("refused at 192.168.1.42:8080 (timeout 30s)"), [["ip", "192.168.1.42:8080"]]);
});

test("ignores look-alikes", () => {
  assert.deepEqual(found("version 1.2.3 on 2026-10-29 at 10:45"), []);
  assert.deepEqual(found("bad ip 999.1.1.1"), []);
  assert.deepEqual(found("not a card 1234 5678 9012 3456"), [], "fails Luhn, and 16 digits is too long for a phone");
  assert.deepEqual(found("a_very_long_identifier_name without digits"), []);
});

test("a word is only claimed once", () => {
  // The IP also looks like a phone number; it should come back once, as an IP.
  assert.deepEqual(found("10.20.30.40"), [["ip", "10.20.30.40"]]);
});
