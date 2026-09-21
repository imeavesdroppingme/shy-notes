import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

type Snapshot = {
  version: number;
  text: string;
  pose: { x: number; y: number; w: number; h: number };
  pinned: boolean;
};

const appEl = () => document.getElementById("app")!;
const editor = () => document.getElementById("editor") as HTMLTextAreaElement;
const pinBtn = () => document.getElementById("pin-btn") as HTMLButtonElement;
const a11y = () => document.getElementById("a11y")!;

let saveTimer: number | undefined;
let pinned = false;

function setPinnedUi(value: boolean) {
  pinned = value;
  const btn = pinBtn();
  btn.setAttribute("aria-pressed", value ? "true" : "false");
  btn.textContent = value ? "●" : "○";
  btn.title = value ? "Unpin" : "Pin";
}

function scheduleSave() {
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => {
    void invoke("save_text", { text: editor().value });
  }, 350);
}

async function init() {
  const snap = await invoke<Snapshot>("get_initial_state");
  editor().value = snap.text ?? "";
  setPinnedUi(!!snap.pinned);

  const win = getCurrentWindow();

  editor().addEventListener("input", scheduleSave);
  window.addEventListener("beforeunload", () => {
    void invoke("save_text", { text: editor().value });
  });

  pinBtn().addEventListener("click", async () => {
    const next = await invoke<boolean>("set_pinned", { pinned: !pinned });
    setPinnedUi(next);
  });

  // Manual window drag via chrome — notify core so repulsion stays suppressed.
  document.querySelector(".chrome")?.addEventListener("mousedown", async () => {
    await invoke("begin_drag");
    try {
      await win.startDragging();
    } finally {
      await invoke("end_drag");
    }
  });

  // Size/position stay owned by the Rust loop in physical pixels.
  // Do not push logical webview sizes — that breaks capture hit-testing on Retina.

  await listen<{ preCapture: boolean }>("visual-hints", (ev) => {
    appEl().classList.toggle("pre-capture", !!ev.payload.preCapture);
  });

  await listen("request-focus", () => {
    const el = editor();
    if (document.activeElement !== el) {
      el.focus();
    }
  });

  await listen<boolean>("pinned-changed", (ev) => {
    setPinnedUi(!!ev.payload);
  });

  await listen("accessibility-needed", () => {
    a11y().classList.remove("hidden");
  });

  document.getElementById("a11y-open")?.addEventListener("click", () => {
    void invoke("open_accessibility_settings");
  });
  document.getElementById("a11y-dismiss")?.addEventListener("click", () => {
    a11y().classList.add("hidden");
  });

  await win.onFocusChanged(async ({ payload: focused }) => {
    if (!focused) return;
    const ok = await invoke<boolean>("check_accessibility");
    if (ok) a11y().classList.add("hidden");
  });

  const ok = await invoke<boolean>("check_accessibility");
  if (!ok) a11y().classList.remove("hidden");
}

void init();
