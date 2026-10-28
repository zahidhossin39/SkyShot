// History search. Pure, so it's unit-tested (search.test.ts).

const words = (q: string) => q.toLowerCase().split(/\s+/).filter(Boolean);

/** Every word of the query appears somewhere in the text (any order, any case). */
export function matches(text: string, query: string): boolean {
  const t = text.toLowerCase();
  return words(query).every((w) => t.includes(w));
}

/** Text around the first query word, split as [before, match, after], to show why a shot matched. */
export function snippet(text: string, query: string, radius = 40): [string, string, string] | null {
  const w = words(query)[0];
  const i = w ? text.toLowerCase().indexOf(w) : -1;
  if (i < 0) return null;
  const flat = text.replace(/\s/g, " "); // same length, so indexes still line up
  const s = Math.max(0, i - radius);
  const e = Math.min(text.length, i + w.length + radius);
  return [
    (s > 0 ? "…" : "") + flat.slice(s, i),
    flat.slice(i, i + w.length),
    flat.slice(i + w.length, e) + (e < text.length ? "…" : ""),
  ];
}
