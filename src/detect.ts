// Find things in OCR text: sensitive text for Smart Redact, links/emails/phones for
// Smart actions. Pure, so it's unit-tested (detect.test.ts). Matches run on a whole
// OCR line, because a phone or card number is usually split into several "words".

export type Word = { x: number; y: number; w: number; h: number; text: string };
export type Kind = "email" | "ip" | "card" | "phone" | "key";
export type Find = { kind: Kind; words: Word[] };

const digits = (m: string) => m.replace(/\D/g, "").length;

/** Card numbers pass the Luhn checksum; random 16-digit runs mostly don't. */
function luhn(m: string) {
  const d = m.replace(/\D/g, "");
  let sum = 0;
  for (let i = 0; i < d.length; i++) {
    let n = +d[d.length - 1 - i];
    if (i % 2) n = n * 2 > 9 ? n * 2 - 9 : n * 2;
    sum += n;
  }
  return sum % 10 === 0;
}

// Order matters: a word claimed by an earlier pattern isn't matched again.
const EMAIL = /[\w.+-]+@[\w-]+(?:\.[\w-]+)+/g;
const PHONE = /\+?\(?\d[\d ().-]{6,}\d/g;
const isPhone = (m: string) => digits(m) >= (m[0] === "+" ? 8 : 9) && digits(m) <= 15;

const PATTERNS: [Kind, RegExp, ((m: string) => boolean)?][] = [
  ["email", EMAIL],
  ["ip", /\b(?:\d{1,3}\.){3}\d{1,3}\b/g, (m) => m.split(".").every((n) => +n <= 255)],
  ["card", /\b\d(?:[ -]?\d){12,18}\b/g, luhn],
  ["phone", PHONE, isPhone],
  // shortcut: any long letters+digits token counts as a key/token (sk-..., ghp_..., AKIA..., JWT parts);
  // catches some long identifiers too, which is fine since the user reviews every box.
  // No "/" so URL paths don't match. The optional prefix covers OCR splitting "sk-proj -4fG7...".
  ["key", /(?:\b(?:sk|pk|rk|ghp|xox[abprs])[-_][\w-]*\s)?[\w+=-]{20,}/g, (m) => /[a-z]/i.test(m) && /\d/.test(m)],
];

export function findSensitive(lines: Word[][]): Find[] {
  const found: Find[] = [];
  for (const line of lines) {
    // Rebuild the line's text, remembering where each word sits in it.
    let text = "";
    const spans = line.map((w) => {
      const start = text.length;
      text += w.text + " ";
      return [start, start + w.text.length];
    });
    const used = new Set<number>();
    for (const [kind, re, ok] of PATTERNS) {
      for (const m of text.matchAll(re)) {
        if (ok && !ok(m[0])) continue;
        const end = m.index! + m[0].length;
        const hit = spans.flatMap(([s, e], i) => (s < end && e > m.index! ? [i] : []));
        if (hit.some((i) => used.has(i))) continue;
        hit.forEach((i) => used.add(i));
        found.push({ kind, words: hit.map((i) => line[i]) });
      }
    }
  }
  return found;
}

export type Action = { kind: "url" | "email" | "phone"; value: string };

/** Smart actions: links to open, emails and phone numbers to copy. Deduped, in reading order. */
export function findActions(text: string): Action[] {
  const out: Action[] = [];
  const add = (kind: Action["kind"], value: string) =>
    out.some((a) => a.value === value) || out.push({ kind, value });
  // Ends on a letter, digit or "/" so trailing punctuation ("see example.com.") is left out.
  for (const m of text.matchAll(/\b(?:https?:\/\/|www\.)[^\s<>"']*[\w/]/gi)) {
    add("url", /^www\./i.test(m[0]) ? `https://${m[0]}` : m[0]);
  }
  for (const m of text.matchAll(EMAIL)) add("email", m[0]);
  for (const m of text.matchAll(PHONE)) if (isPhone(m[0])) add("phone", m[0]);
  return out;
}
