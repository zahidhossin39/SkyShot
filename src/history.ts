// History window: every finished capture, searchable by the text in it.

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { matches, snippet } from "./search";

type Entry = { id: string; created: number; width: number; height: number; text: string; starred: boolean };

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const grid = $("grid");
const q = $<HTMLInputElement>("q");
const onlyStarred = $<HTMLInputElement>("only-starred");
const count = $("count");
const empty = $("empty");
const toast = $("toast");

// Theme chosen in Settings (shared localStorage); the <head> script handles first paint.
function syncTheme() {
  let t: string | null = null;
  try {
    t = localStorage.getItem("shutter-theme");
  } catch {}
  if (t === "light" || t === "dark") document.documentElement.dataset.theme = t;
  else delete document.documentElement.dataset.theme;
  try {
    getCurrentWindow().setTheme(t === "light" || t === "dark" ? t : null).catch(() => {}); // title bar too
  } catch {}
}
window.addEventListener("storage", (e) => {
  if (e.key === "shutter-theme" || e.key === null) syncTheme();
});
syncTheme(); // the <head> script painted the page; this also sets the title bar

let entries: Entry[] = [];
let toastTimer = 0;

function say(text: string) {
  toast.textContent = text;
  toast.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (toast.hidden = true), 1400);
}

const DAY = 86_400_000;
function when(ms: number) {
  const d = new Date(ms);
  const time = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  const today = new Date().setHours(0, 0, 0, 0);
  if (ms >= today) return `Today ${time}`;
  if (ms >= today - DAY) return `Yesterday ${time}`;
  const sameYear = d.getFullYear() === new Date().getFullYear();
  return `${d.toLocaleDateString([], { day: "numeric", month: "short", year: sameYear ? undefined : "numeric" })} ${time}`;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, props: Partial<HTMLElementTagNameMap[K]> = {}, ...kids: (Node | string)[]) {
  const e = Object.assign(document.createElement(tag), props);
  e.append(...kids);
  return e;
}

/** History images come through the app's own `shot` protocol (see lib.rs). */
const shot = (name: string) => convertFileSrc(name, "shot");

function card(e: Entry, query: string) {
  const img = el("img", { loading: "lazy", alt: "", src: shot(`${e.id}.thumb.png`) });
  const thumb = el("button", { className: "thumb", title: "View" }, img);
  thumb.onclick = () => view(e);

  const star = el("button", { className: "star", title: e.starred ? "Starred: kept forever" : "Star to keep forever" });
  star.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3.6l2.6 5.3 5.8.8-4.2 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.2-4.1 5.8-.8z" /></svg>';
  star.setAttribute("aria-pressed", String(e.starred));
  star.setAttribute("aria-label", "Star");
  star.onclick = async () => {
    await invoke("history_star", { id: e.id, starred: !e.starred });
    e.starred = !e.starred;
    render();
  };

  const meta = el("div", { className: "meta" }, el("time", { textContent: when(e.created) }), el("span", { className: "dims", textContent: `${e.width}×${e.height}` }), star);

  const parts = query ? snippet(e.text, query) : null;
  const snip = parts && el("p", { className: "snip" }, parts[0], el("mark", { textContent: parts[1] }), parts[2]);

  const copyBtn = el("button", { textContent: "Copy" });
  copyBtn.onclick = () => copy(e);
  const reveal = el("button", { textContent: "Show in folder" });
  reveal.onclick = () => invoke("history_reveal", { id: e.id }).catch((err) => say(String(err)));
  // Two clicks to delete, without a dialog: the first arms it for 3 s.
  const del = el("button", { className: "del", textContent: "Delete" });
  let armed = 0;
  del.onclick = async () => {
    if (!armed) {
      del.textContent = "Sure?";
      del.classList.add("confirm");
      armed = window.setTimeout(() => {
        armed = 0;
        del.textContent = "Delete";
        del.classList.remove("confirm");
      }, 3000);
      return;
    }
    clearTimeout(armed);
    await invoke("history_delete", { id: e.id });
    entries = entries.filter((x) => x.id !== e.id);
    render();
  };

  return el("article", { className: "card" }, thumb, meta, ...(snip ? [snip] : []), el("div", { className: "actions" }, copyBtn, reveal, del));
}

async function copy(e: Entry) {
  try {
    await invoke("history_copy", { id: e.id });
    say("Image copied");
  } catch (err) {
    say(String(err));
  }
}

// ---------------------------------------------------------------- viewer
// Full image right here, not in Explorer. ← → step through what the grid shows.

const viewer = $("viewer");
const viewerImg = $<HTMLImageElement>("viewer-img");
let viewing: Entry | null = null;
let shown: Entry[] = [];

function view(e: Entry | null) {
  viewing = e;
  viewer.hidden = !e;
  if (!e) return;
  viewerImg.src = shot(`${e.id}.png`);
  const i = shown.indexOf(e);
  $("viewer-info").textContent = `${when(e.created)} · ${e.width}×${e.height}`;
  $<HTMLButtonElement>("viewer-prev").disabled = i <= 0;
  $<HTMLButtonElement>("viewer-next").disabled = i < 0 || i >= shown.length - 1;
}

function step(d: number) {
  const i = viewing ? shown.indexOf(viewing) : -1;
  if (i >= 0 && shown[i + d]) view(shown[i + d]);
}

$("viewer-prev").onclick = () => step(-1);
$("viewer-next").onclick = () => step(1);
$("viewer-copy").onclick = () => viewing && copy(viewing);
$("viewer-close").onclick = () => view(null);
viewer.addEventListener("click", (ev) => ev.target === viewer && view(null)); // click the backdrop

function render() {
  const query = q.value.trim();
  shown = entries.filter((e) => (!onlyStarred.checked || e.starred) && matches(e.text, query));
  grid.replaceChildren(...shown.map((e) => card(e, query)));
  count.textContent = query || onlyStarred.checked ? `${shown.length} of ${entries.length}` : `${entries.length} shots`;
  empty.hidden = shown.length > 0;
  empty.textContent = entries.length
    ? "Nothing matches. Search looks at the text in each screenshot (hidden or redacted text isn't included)."
    : "No screenshots yet. Everything you copy or save shows up here, searchable by its text.";
}

async function load() {
  const view = await invoke<{ entries: Entry[] }>("history_list");
  entries = view.entries;
  render();
}

q.addEventListener("input", render);
onlyStarred.addEventListener("change", render);
window.addEventListener("keydown", (e) => {
  if (viewing) {
    if (e.key === "Escape") view(null);
    else if (e.key === "ArrowLeft") step(-1);
    else if (e.key === "ArrowRight") step(1);
    else if (e.ctrlKey && e.key.toLowerCase() === "c") copy(viewing);
    else return;
    return e.preventDefault();
  }
  if ((e.ctrlKey && e.key.toLowerCase() === "f") || (e.key === "/" && document.activeElement !== q)) {
    e.preventDefault();
    q.focus();
    q.select();
  }
  if (e.key === "Escape" && q.value) {
    q.value = "";
    render();
  }
});
listen("history:changed", load);
load().then(() => q.focus());
