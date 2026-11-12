// Pinned screenshot: an always-on-top reference window.
// Drag to move, wheel to zoom, Ctrl+C copies, Esc / double-click / × closes.

import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, PhysicalSize } from "@tauri-apps/api/window";

const win = getCurrentWindow();
const img = document.getElementById("img") as HTMLImageElement;
const toast = document.getElementById("toast")!;
let zoom = 1;
let toastTimer = 0;

function say(text: string) {
  toast.textContent = text;
  toast.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (toast.hidden = true), 900);
}

async function load() {
  const png = await invoke<ArrayBuffer>("pin_image");
  img.src = URL.createObjectURL(new Blob([png], { type: "image/png" }));
  await img.decode();
  await win.show();
  await win.setFocus();
}

// Window dragging swallows the mouse-up, so the browser never sees a double-click
// (e.detail stays 1). Time the presses ourselves.
let lastDown = 0;
img.addEventListener("mousedown", (e) => {
  if (e.button !== 0) return;
  if (e.timeStamp - lastDown < 400) return void win.close();
  lastDown = e.timeStamp;
  void win.startDragging();
});

// Zoom by resizing the window around the image's own pixel size (naturalWidth = physical px).
window.addEventListener(
  "wheel",
  (e) => {
    e.preventDefault();
    zoom = Math.min(5, Math.max(0.2, zoom * (e.deltaY < 0 ? 1.1 : 1 / 1.1)));
    const w = Math.max(40, Math.round(img.naturalWidth * zoom));
    const h = Math.max(40, Math.round(img.naturalHeight * zoom));
    void win.setSize(new PhysicalSize(w, h));
    say(`${Math.round(zoom * 100)}%`);
  },
  { passive: false },
);

window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") void win.close();
  if (e.ctrlKey && e.key.toLowerCase() === "c") {
    invoke("pin_copy").then(() => say("Copied"), (err) => say(String(err)));
  }
  if (e.key === "0" || (e.ctrlKey && e.key === "0")) {
    zoom = 1;
    void win.setSize(new PhysicalSize(img.naturalWidth, img.naturalHeight));
    say("100%");
  }
});

document.getElementById("close")!.addEventListener("click", () => void win.close());
document.addEventListener("contextmenu", (e) => e.preventDefault());

load().catch((e) => console.error("[pin] load failed", e));
