import { invoke } from "@tauri-apps/api/core";

type Action = "capture" | "text";
type SettingsView = {
  hotkeyCapture: string;
  hotkeyText: string;
  captureActive: boolean;
  textActive: boolean;
  saveDir: string;
  saveDirIsDefault: boolean;
  autostart: boolean;
  historyDays: number;
};

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const errorEl = $("hotkey-error");
const buttons = document.querySelectorAll<HTMLButtonElement>("button.hotkey");

/** "Ctrl+PrintScreen" -> "Ctrl + PrtSc", "Alt+KeyS" -> "Alt + S" */
function pretty(accel: string): string {
  return accel
    .split("+")
    .map((p) =>
      p === "PrintScreen" ? "PrtSc"
      : p === "Super" ? "Win"
      : /^Key[A-Z]$/.test(p) ? p.slice(3)
      : /^Digit\d$/.test(p) ? p.slice(5)
      : p.replace(/^Arrow/, ""),
    )
    .join(" + ");
}

let current: SettingsView;

function render(v: SettingsView) {
  current = v;
  buttons.forEach((b) => {
    const action = b.dataset.action as Action;
    b.textContent = pretty(action === "capture" ? v.hotkeyCapture : v.hotkeyText);
    b.classList.remove("recording");
  });
  setStatus($("capture-status"), v.captureActive);
  setStatus($("text-status"), v.textActive);
  $("save-dir").textContent = v.saveDir;
  $("save-dir").title = v.saveDir;
  $("reset-dir").hidden = v.saveDirIsDefault;
  $<HTMLInputElement>("autostart").checked = v.autostart;
  const days = $<HTMLSelectElement>("history-days");
  // A value set outside the presets (hand-edited settings.json) still shows up.
  if (![...days.options].some((o) => o.value === String(v.historyDays))) {
    days.add(new Option(`${v.historyDays} days`, String(v.historyDays)));
  }
  days.value = String(v.historyDays);
}

function setStatus(el: HTMLElement, active: boolean) {
  el.className = `status ${active ? "ok" : "bad"}`;
  el.textContent = active ? "Active" : "Not active — another app is using this shortcut";
}

function showError(msg: string) {
  errorEl.textContent = msg;
  errorEl.hidden = !msg;
}

async function refresh() {
  render(await invoke<SettingsView>("get_settings"));
}

// ---------------------------------------------------------------- hotkey recorder
// While recording, global hotkeys are paused so pressing the current combo doesn't
// trigger a capture.

let recording: { action: Action; button: HTMLButtonElement } | null = null;

const MODIFIER = /^(Control|Shift|Alt|Meta|OS)(Left|Right)?$/;

function mods(e: KeyboardEvent) {
  const m: string[] = [];
  if (e.ctrlKey) m.push("Ctrl");
  if (e.altKey) m.push("Alt");
  if (e.shiftKey) m.push("Shift");
  if (e.metaKey) m.push("Super");
  return m;
}

async function startRecording(button: HTMLButtonElement) {
  if (recording) await stopRecording();
  showError("");
  await invoke("pause_hotkeys", { paused: true });
  recording = { action: button.dataset.action as Action, button };
  button.classList.add("recording");
  button.textContent = "Press keys…";
}

async function stopRecording() {
  recording = null;
  await invoke("pause_hotkeys", { paused: false });
  await refresh();
}

async function commit(e: KeyboardEvent) {
  if (!recording) return;
  const { action } = recording;
  const accel = [...mods(e), e.code].join("+");
  try {
    render(await invoke<SettingsView>("set_hotkey", { action, accel }));
  } catch (err) {
    showError(String(err));
  }
  await stopRecording();
}

window.addEventListener(
  "keydown",
  (e) => {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape" && mods(e).length === 0) return void stopRecording();
    if (MODIFIER.test(e.code)) {
      recording.button.textContent = `${mods(e).map((m) => pretty(m)).join(" + ")} + …`;
      return;
    }
    if (e.code === "PrintScreen") return; // Windows only delivers PrtSc on keyup
    commit(e);
  },
  true,
);

window.addEventListener(
  "keyup",
  (e) => {
    if (recording && e.code === "PrintScreen") {
      e.preventDefault();
      commit(e);
    }
  },
  true,
);

window.addEventListener("blur", () => {
  if (recording) stopRecording();
});

buttons.forEach((b) => b.addEventListener("click", () => startRecording(b)));

$("reset-hotkeys").addEventListener("click", async () => {
  showError("");
  render(await invoke<SettingsView>("reset_hotkeys"));
});

// ---------------------------------------------------------------- saving & general

$("open-dir").addEventListener("click", () => invoke("open_save_dir"));
$("change-dir").addEventListener("click", async () => render(await invoke<SettingsView>("pick_save_dir")));
$("reset-dir").addEventListener("click", async () => render(await invoke<SettingsView>("reset_save_dir")));

$<HTMLInputElement>("autostart").addEventListener("change", async (e) => {
  const enabled = (e.target as HTMLInputElement).checked;
  try {
    render(await invoke<SettingsView>("set_autostart", { enabled }));
  } catch (err) {
    $<HTMLInputElement>("autostart").checked = current.autostart;
    showError(String(err));
  }
});

$<HTMLSelectElement>("history-days").addEventListener("change", async (e) => {
  const select = e.target as HTMLSelectElement;
  const days = Number(select.value);
  const shorter = days !== 0 && (current.historyDays === 0 || days < current.historyDays);
  // Shortening deletes older (unstarred) shots right away, so check first.
  if (shorter && !confirm(`Delete unstarred screenshots older than ${select.selectedOptions[0].text}?`)) {
    select.value = String(current.historyDays);
    return;
  }
  try {
    render(await invoke<SettingsView>("set_history_days", { days }));
  } catch (err) {
    select.value = String(current.historyDays);
    showError(String(err));
  }
});
$("open-history").addEventListener("click", () => invoke("open_history_window"));

refresh();
