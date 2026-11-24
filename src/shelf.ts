// The shelf window: thumbnails of pinned shots. Drag one into any app (native OS drag of its
// PNG, via tauri-plugin-drag); a successful drop removes it. Rust owns the list and the files.

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { startDrag } from "@crabnebula/tauri-plugin-drag";

type Item = { id: string; width: number; height: number };

const items = document.getElementById("items")!;
const grip = document.getElementById("grip")!;

// Theme chosen in Settings (shared localStorage); the <head> script handles first paint.
function syncTheme() {
  let t: string | null = null;
  try {
    t = localStorage.getItem("shutter-theme");
  } catch {}
  if (t === "light" || t === "dark") document.documentElement.dataset.theme = t;
  else delete document.documentElement.dataset.theme;
}
window.addEventListener("storage", (e) => {
  if (e.key === "shutter-theme" || e.key === null) syncTheme();
});

/** Shelf thumbnails come through the app's own `shelf` protocol (see shelf.rs). */
const thumb = (id: string) => convertFileSrc(`${id}.thumb.png`, "shelf");

const DRAG_START_PX = 5;

function card(item: Item) {
  const div = document.createElement("div");
  div.className = "item";
  div.dataset.id = item.id;
  div.title = `${item.width} × ${item.height}. Drag into any app.`;
  const img = document.createElement("img");
  img.src = thumb(item.id);
  img.alt = "Pinned screenshot";
  img.draggable = false;
  img.style.aspectRatio = `${item.width} / ${item.height}`;
  const x = document.createElement("button");
  x.className = "x";
  x.textContent = "×";
  x.title = "Remove";
  x.setAttribute("aria-label", "Remove from shelf");
  x.addEventListener("pointerdown", (e) => e.stopPropagation());
  x.addEventListener("click", (e) => {
    e.stopPropagation();
    void invoke("shelf_remove", { id: item.id, dropped: false });
  });
  div.append(img, x);

  // A press only becomes an OS drag after the pointer moves a few px, so a plain click does nothing.
  div.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    const [sx, sy] = [e.clientX, e.clientY];
    const move = (m: PointerEvent) => {
      if (Math.hypot(m.clientX - sx, m.clientY - sy) < DRAG_START_PX) return;
      cleanup();
      void drag(item, div);
    };
    const cleanup = () => {
      removeEventListener("pointermove", move);
      removeEventListener("pointerup", cleanup);
      removeEventListener("pointercancel", cleanup);
    };
    addEventListener("pointermove", move);
    addEventListener("pointerup", cleanup);
    addEventListener("pointercancel", cleanup);
  });
  return div;
}

async function drag(item: Item, div: HTMLElement) {
  div.classList.add("dragging");
  try {
    const [file, icon] = await Promise.all([
      invoke<string>("shelf_path", { name: `${item.id}.png` }),
      invoke<string>("shelf_path", { name: `${item.id}.thumb.png` }),
    ]);
    await startDrag({ item: [file], icon, mode: "copy" }, (r) => {
      // Dropped: gone from the shelf. Cancelled: it stays.
      if (r.result === "Dropped") void invoke("shelf_remove", { id: item.id, dropped: true });
      else div.classList.remove("dragging");
    });
  } catch (e) {
    console.error("[shelf] drag failed", e);
    div.classList.remove("dragging");
  }
}

let drawn = "";
async function render() {
  const list = await invoke<Item[]>("shelf_list");
  const key = list.map((i) => i.id).join(",");
  if (key !== drawn) {
    drawn = key;
    items.replaceChildren(...list.map(card));
  }
  // Tell Rust how tall we want to be: it sizes, docks and shows (or hides, when empty) the window.
  await new Promise((r) => requestAnimationFrame(r));
  await invoke("shelf_fit", { height: grip.offsetHeight + items.scrollHeight + 2 + 16 }); // + border + margins
}

// Images load async, so the height can grow after the first fit: refit when any one loads.
items.addEventListener("load", () => void render(), true);
listen("shelf:changed", () => void render());
document.getElementById("clear")!.addEventListener("click", () => void invoke("shelf_clear"));
document.addEventListener("contextmenu", (e) => e.preventDefault());
syncTheme();
void render();
