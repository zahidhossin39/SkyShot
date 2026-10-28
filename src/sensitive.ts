// Smart Redact: find sensitive text in OCR word boxes. Pure, so it's unit-tested
// (sensitive.test.ts). Matches run on a whole OCR line, because a phone or card
// number is usually split into several "words".

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
const PATTERNS: [Kind, RegExp, ((m: string) => boolean)?][] = [
  ["email", /[\w.+-]+@[\w-]+(?:\.[\w-]+)+/g],
  ["ip", /\b(?:\d{1,3}\.){3}\d{1,3}\b/g, (m) => m.split(".").every((n) => +n <= 255)],
  ["card", /\b\d(?:[ -]?\d){12,18}\b/g, luhn],
  ["phone", /\+?\(?\d[\d ().-]{6,}\d/g, (m) => digits(m) >= (m[0] === "+" ? 8 : 9) && digits(m) <= 15],
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
