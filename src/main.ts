import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openUrl } from "@tauri-apps/plugin-opener";

type NoteMeta = {
  id: string;
  title?: string | null;
  created_at: string;
  updated_at: string;
  background: string;
  foreground?: string;
  font_size?: number;
};

type NoteSummary = {
  id: string;
  label: string;
  background?: string;
  foreground?: string;
};

type UserPrefs = {
  repulsion_strength: number;
  influence_radius: number;
  capture_diameter: number;
  show_glow: boolean;
  show_line_numbers: boolean;
  use_monospace: boolean;
  open_at_startup: boolean;
};

type ActiveNoteDto = {
  notes: NoteSummary[];
  active_note_id: string;
  text: string;
  note: NoteMeta;
};

type Snapshot = {
  version: number;
  text: string;
  pose: { x: number; y: number; w: number; h: number };
  pinned: boolean;
  note: NoteMeta;
  prefs: UserPrefs;
  notes: NoteSummary[];
  active_note_id: string;
};

const NEW_NOTE_VALUE = "__new__";
const THEME_DEFAULT_BG = "#f3efe6";
const THEME_DEFAULT_FG = "#1c1a16";

const appEl = () => document.getElementById("app")!;
const editor = () => document.getElementById("editor") as HTMLTextAreaElement;
const pinBtn = () => document.getElementById("pin-btn") as HTMLButtonElement;
const closeBtn = () => document.getElementById("close-btn") as HTMLButtonElement;
const settingsBtn = () => document.getElementById("settings-btn") as HTMLButtonElement;
const menuBtn = () => document.getElementById("menu-btn") as HTMLButtonElement;
const noteComboTrigger = () => document.getElementById("note-combo-trigger") as HTMLButtonElement;
const noteComboSwatch = () => document.getElementById("note-combo-swatch") as HTMLElement;
const noteComboLabel = () => document.getElementById("note-combo-label")!;
const noteComboList = () => document.getElementById("note-combo-list")!;
const appMenu = () => document.getElementById("app-menu")!;
const a11y = () => document.getElementById("a11y")!;
const glowEl = () => document.getElementById("capture-glow")!;
const gutter = () => document.getElementById("line-gutter")!;

let saveTimer: number | undefined;
let lineRefreshTimer: number | undefined;
let pinned = false;
let pinInFlight = false;
let showGlow = true;
let showLineNumbers = false;
let useMonospace = false;
let activeNoteId = "";
let switchingNote = false;
let lineMirror: HTMLDivElement | null = null;
let noteSummaries: NoteSummary[] = [];

function resolveColors(background?: string, foreground?: string) {
  const bg =
    background && /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(background)
      ? background
      : THEME_DEFAULT_BG;
  const fg =
    foreground && /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(foreground)
      ? foreground
      : THEME_DEFAULT_FG;
  return { bg, fg };
}

function setPinnedUi(value: boolean) {
  pinned = value;
  const btn = pinBtn();
  btn.setAttribute("aria-pressed", value ? "true" : "false");
  btn.title = value ? "Unpin" : "Pin";
}

async function togglePinned() {
  if (pinInFlight) return;
  pinInFlight = true;
  const desired = !pinned;
  setPinnedUi(desired);
  console.info("[shy-notes] pin →", desired);
  try {
    const next = await Promise.race([
      invoke<boolean>("set_pinned", { pinned: desired }),
      new Promise<never>((_, reject) => {
        window.setTimeout(() => reject(new Error("set_pinned timeout")), 2000);
      }),
    ]);
    setPinnedUi(next);
    console.info("[shy-notes] pin ok →", next);
  } catch (err) {
    console.warn("[shy-notes] pin failed", err);
    // Keep optimistic UI: native side may have applied before a stalled reply.
  } finally {
    pinInFlight = false;
  }
}

function setGlow(intensity: number, preCapture: boolean) {
  const el = glowEl();
  if (!showGlow) {
    el.classList.remove("visible");
    appEl().classList.remove("pre-capture");
    return;
  }
  const glow = Math.max(0, Math.min(1, intensity));
  el.style.setProperty("--glow-intensity", String(glow));
  el.classList.toggle("visible", glow > 0.02);
  appEl().classList.toggle("pre-capture", preCapture);
}

function applyNoteMeta(note: NoteMeta) {
  if (note.background) {
    appEl().style.setProperty("--note-bg", note.background);
  } else {
    appEl().style.removeProperty("--note-bg");
  }
  if (note.foreground) {
    appEl().style.setProperty("--note-fg", note.foreground);
  } else {
    appEl().style.removeProperty("--note-fg");
  }
  const size = Math.max(10, Math.min(28, Number(note.font_size) || 15));
  appEl().style.setProperty("--note-font-size", `${size}px`);
  const { bg } = resolveColors(note.background, note.foreground);
  noteComboSwatch().style.background = bg;
  scheduleLineNumbers();
}

function setComboOpen(open: boolean) {
  noteComboList().classList.toggle("hidden", !open);
  noteComboTrigger().setAttribute("aria-expanded", open ? "true" : "false");
}

function populateNoteSelect(notes: NoteSummary[], activeId: string) {
  noteSummaries = notes;
  const list = noteComboList();
  list.replaceChildren();

  for (const n of notes) {
    const { bg } = resolveColors(n.background, n.foreground);
    const opt = document.createElement("button");
    opt.type = "button";
    opt.className = "note-combo-option";
    opt.role = "option";
    opt.dataset.id = n.id;
    opt.setAttribute("aria-selected", n.id === activeId ? "true" : "false");

    const swatch = document.createElement("span");
    swatch.className = "note-combo-swatch";
    swatch.setAttribute("aria-hidden", "true");
    swatch.style.background = bg;

    const label = document.createElement("span");
    label.className = "note-combo-option-label";
    label.textContent = n.label;

    opt.append(swatch, label);
    list.appendChild(opt);
  }

  const create = document.createElement("button");
  create.type = "button";
  create.className = "note-combo-option note-combo-create";
  create.role = "option";
  create.dataset.id = NEW_NOTE_VALUE;
  create.textContent = "＋ New note";
  list.appendChild(create);

  const active = notes.find((n) => n.id === activeId) ?? notes[0];
  activeNoteId = active?.id ?? "";
  noteComboLabel().textContent = active?.label ?? "Untitled note";
  if (active) {
    const { bg } = resolveColors(active.background, active.foreground);
    noteComboSwatch().style.background = bg;
  }
}

function applyActiveNote(payload: ActiveNoteDto, updateEditor: boolean) {
  activeNoteId = payload.active_note_id;
  populateNoteSelect(payload.notes, payload.active_note_id);
  applyNoteMeta(payload.note);
  if (updateEditor) {
    editor().value = payload.text ?? "";
    scheduleLineNumbers();
  }
}

function applyPrefs(prefs: UserPrefs) {
  showGlow = !!prefs.show_glow;
  showLineNumbers = !!prefs.show_line_numbers;
  useMonospace = !!prefs.use_monospace;
  gutter().classList.toggle("hidden", !showLineNumbers);
  editor().classList.toggle("monospace", useMonospace);
  gutter().classList.toggle("monospace", useMonospace);
  if (!showGlow) setGlow(0, false);
  scheduleLineNumbers();
}

function ensureLineMirror(): HTMLDivElement {
  if (lineMirror && lineMirror.isConnected) return lineMirror;
  const mirror = document.createElement("div");
  mirror.id = "line-mirror";
  mirror.setAttribute("aria-hidden", "true");
  mirror.className = "line-mirror";
  document.body.appendChild(mirror);
  lineMirror = mirror;
  return mirror;
}

function refreshLineNumbers() {
  const g = gutter();
  if (!showLineNumbers) {
    g.replaceChildren();
    return;
  }

  const ed = editor();
  const logicalLines = ed.value.split("\n");
  const mirror = ensureLineMirror();
  const cs = getComputedStyle(ed);

  mirror.style.boxSizing = "border-box";
  mirror.style.width = `${ed.clientWidth}px`;
  mirror.style.font = cs.font;
  mirror.style.letterSpacing = cs.letterSpacing;
  mirror.style.wordSpacing = cs.wordSpacing;
  mirror.style.lineHeight = cs.lineHeight;
  mirror.style.paddingLeft = cs.paddingLeft;
  mirror.style.paddingRight = cs.paddingRight;
  mirror.style.paddingTop = "0";
  mirror.style.paddingBottom = "0";
  mirror.style.border = "0";
  mirror.style.whiteSpace = "pre-wrap";
  mirror.style.overflowWrap = "break-word";
  mirror.style.wordBreak = cs.wordBreak;

  const frag = document.createDocumentFragment();
  for (let i = 0; i < logicalLines.length; i++) {
    const raw = logicalLines[i];
    mirror.textContent = raw.length === 0 ? " " : raw;
    const height = Math.max(mirror.offsetHeight, parseFloat(cs.lineHeight) || 18);

    const row = document.createElement("div");
    row.className = "ln";
    row.textContent = String(i + 1);
    row.style.height = `${height}px`;
    frag.appendChild(row);
  }
  g.replaceChildren(frag);
  g.scrollTop = ed.scrollTop;
}

function scheduleLineNumbers() {
  window.clearTimeout(lineRefreshTimer);
  lineRefreshTimer = window.setTimeout(() => refreshLineNumbers(), 16);
}

function scheduleSave() {
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => {
    void invoke("save_text", { text: editor().value });
  }, 700);
  scheduleLineNumbers();
}

function flushSave() {
  window.clearTimeout(saveTimer);
  void invoke("save_text", { text: editor().value });
}

function setMenuOpen(open: boolean) {
  appMenu().classList.toggle("hidden", !open);
  menuBtn().setAttribute("aria-expanded", open ? "true" : "false");
}

function closeMenu() {
  setMenuOpen(false);
}

/** URL under caret/click index (http/https, strips trailing punctuation). */
function urlAtIndex(text: string, index: number): string | null {
  const re = /https?:\/\/[^\s<>"'`]+/gi;
  let match: RegExpExecArray | null;
  while ((match = re.exec(text)) !== null) {
    const start = match.index;
    const trimmed = match[0].replace(/[.,;:!?)\]}>]+$/g, "");
    const end = start + trimmed.length;
    if (index >= start && index <= end) return trimmed;
  }
  return null;
}

function openUrlAtCaret(ed: HTMLTextAreaElement): boolean {
  const url = urlAtIndex(ed.value, ed.selectionStart);
  if (!url) return false;
  // Never await opener on the UI path — it can stall the whole webview.
  void openUrl(url).catch((err) => {
    console.warn("[shy-notes] openUrl failed", err);
  });
  return true;
}

async function createNote() {
  flushSave();
  const payload = await invoke<ActiveNoteDto>("create_note");
  applyActiveNote(payload, true);
  editor().focus();
}

async function onNoteSelectChange(value: string) {
  if (switchingNote) return;
  if (value === NEW_NOTE_VALUE) {
    switchingNote = true;
    setComboOpen(false);
    try {
      await createNote();
    } finally {
      switchingNote = false;
    }
    return;
  }
  if (value === activeNoteId) {
    setComboOpen(false);
    return;
  }
  switchingNote = true;
  flushSave();
  setComboOpen(false);
  try {
    const payload = await invoke<ActiveNoteDto>("switch_note", { id: value });
    applyActiveNote(payload, true);
  } catch {
    populateNoteSelect(noteSummaries, activeNoteId);
  } finally {
    switchingNote = false;
  }
}

async function init() {
  const snap = await invoke<Snapshot>("get_initial_state");
  editor().value = snap.text ?? "";
  setPinnedUi(!!snap.pinned);
  applyActiveNote(
    {
      notes: snap.notes ?? [],
      active_note_id: snap.active_note_id ?? snap.note?.id ?? "",
      text: snap.text ?? "",
      note: snap.note,
    },
    false,
  );
  applyPrefs(snap.prefs);

  const win = getCurrentWindow();

  editor().addEventListener("input", scheduleSave);
  editor().addEventListener("blur", flushSave);
  editor().addEventListener("scroll", () => {
    gutter().scrollTop = editor().scrollTop;
  });
  // Cmd/Ctrl+click (or middle-click) opens http(s) URLs in the default browser.
  editor().addEventListener("click", (ev) => {
    if (!(ev.metaKey || ev.ctrlKey)) return;
    if (openUrlAtCaret(editor())) ev.preventDefault();
  });
  editor().addEventListener("auxclick", (ev) => {
    if (ev.button !== 1) return;
    ev.preventDefault();
    openUrlAtCaret(editor());
  });
  window.addEventListener("beforeunload", flushSave);
  window.addEventListener("resize", scheduleLineNumbers);

  const wrap = document.querySelector(".editor-wrap");
  if (wrap && typeof ResizeObserver !== "undefined") {
    new ResizeObserver(() => scheduleLineNumbers()).observe(wrap);
  }

  noteComboTrigger().addEventListener("click", (ev) => {
    ev.stopPropagation();
    setComboOpen(noteComboList().classList.contains("hidden"));
  });
  noteComboTrigger().addEventListener("mousedown", (ev) => ev.stopPropagation());
  noteComboList().addEventListener("mousedown", (ev) => ev.stopPropagation());
  noteComboList().addEventListener("click", (ev) => {
    const btn = (ev.target as HTMLElement | null)?.closest("button[data-id]") as
      | HTMLButtonElement
      | null;
    if (!btn?.dataset.id) return;
    void onNoteSelectChange(btn.dataset.id);
  });
  document.addEventListener("click", (ev) => {
    if (!(ev.target as HTMLElement | null)?.closest("#note-combo")) {
      setComboOpen(false);
    }
  });

  pinBtn().addEventListener("click", (ev) => {
    ev.preventDefault();
    ev.stopPropagation();
    void togglePinned();
  });

  settingsBtn().addEventListener("click", () => {
    closeMenu();
    void invoke("open_settings");
  });

  menuBtn().addEventListener("click", (ev) => {
    ev.stopPropagation();
    setMenuOpen(appMenu().classList.contains("hidden"));
  });

  appMenu().addEventListener("click", (ev) => {
    const target = (ev.target as HTMLElement | null)?.closest("button[data-action]") as
      | HTMLButtonElement
      | null;
    if (!target) return;
    const action = target.dataset.action;
    closeMenu();
    if (action === "new-note") {
      void createNote();
    } else if (action === "settings") {
      void invoke("open_settings");
    } else if (action === "about") {
      void invoke("open_about");
    } else if (action === "pin") {
      void togglePinned();
    } else if (action === "reset") {
      void invoke("reset_note_position");
    } else if (action === "toggle-visibility") {
      void invoke("toggle_window_visibility");
    } else if (action === "quit") {
      void invoke("quit_app");
    }
  });

  document.addEventListener("click", (ev) => {
    const menu = (ev.target as HTMLElement | null)?.closest(".menu-wrap");
    if (!menu) closeMenu();
  });
  document.addEventListener("keydown", (ev) => {
    if (ev.key === "Escape") {
      closeMenu();
      setComboOpen(false);
    }
  });

  closeBtn().addEventListener("click", () => {
    flushSave();
    void invoke("hide_window");
  });

  document.querySelector(".chrome")?.addEventListener("mousedown", async (ev) => {
    const target = ev.target as HTMLElement | null;
    if (
      target?.closest("button") ||
      target?.closest(".app-menu") ||
      target?.closest("#note-combo")
    ) {
      return;
    }
    closeMenu();
    await invoke("begin_drag");
    try {
      await win.startDragging();
    } finally {
      await invoke("end_drag");
    }
  });

  await listen<{ preCapture: boolean; glow: number }>("visual-hints", (ev) => {
    setGlow(ev.payload.glow ?? 0, !!ev.payload.preCapture);
  });

  await listen("request-focus", () => {
    const el = editor();
    if (document.activeElement !== el) el.focus();
  });

  await listen<boolean>("pinned-changed", (ev) => setPinnedUi(!!ev.payload));
  await listen<UserPrefs>("prefs-changed", (ev) => applyPrefs(ev.payload));
  await listen<ActiveNoteDto>("active-note", (ev) => {
    const keepEditor = ev.payload.active_note_id === activeNoteId;
    applyActiveNote(ev.payload, !keepEditor);
    if (keepEditor) {
      // Same note: refresh labels/meta only (e.g. title change from settings).
      populateNoteSelect(ev.payload.notes, ev.payload.active_note_id);
      applyNoteMeta(ev.payload.note);
    }
  });

  await listen<boolean>("visibility-changed", (ev) => {
    const visible = !!ev.payload;
    const item = document.getElementById("visibility-menu-item");
    if (item) item.textContent = visible ? "Hide" : "Show";
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

  refreshLineNumbers();
}

void init();
