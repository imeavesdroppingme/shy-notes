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
  tab_size?: number;
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
let tabSize = 4;
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
  const size = Math.max(FONT_MIN, Math.min(FONT_MAX, Number(note.font_size) || 15));
  currentFontSize = size;
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
    updateDocStats();
  }
}

function applyPrefs(prefs: UserPrefs) {
  showGlow = !!prefs.show_glow;
  showLineNumbers = !!prefs.show_line_numbers;
  useMonospace = !!prefs.use_monospace;
  tabSize = Math.max(2, Math.min(8, Math.round(Number(prefs.tab_size) || 4)));
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
  updateDocStats();
}

function flushSave() {
  window.clearTimeout(saveTimer);
  void invoke("save_text", { text: editor().value });
}

const FONT_MIN = 10;
const FONT_MAX = 28;
let currentFontSize = 15;

type TextMatch = { start: number; end: number; line: number; preview: string };

let findMatches: TextMatch[] = [];
let findIndex = -1;

function findPanel() {
  return document.getElementById("find-panel")!;
}
function findQuery() {
  return document.getElementById("find-query") as HTMLInputElement;
}
function findReplaceInput() {
  return document.getElementById("find-replace") as HTMLInputElement;
}
function findStatus() {
  return document.getElementById("find-status")!;
}
function findResults() {
  return document.getElementById("find-results")!;
}

function updateDocStats() {
  const text = editor().value;
  const lines = text.length === 0 ? 1 : text.split("\n").length;
  const words = (text.trim().match(/\S+/g) || []).length;
  const el = document.getElementById("doc-stats");
  if (el) {
    el.textContent = `${lines} line${lines === 1 ? "" : "s"} · ${words} word${words === 1 ? "" : "s"}`;
  }
}

function setFontSize(size: number, persist: boolean) {
  currentFontSize = Math.max(FONT_MIN, Math.min(FONT_MAX, Math.round(size)));
  appEl().style.setProperty("--note-font-size", `${currentFontSize}px`);
  scheduleLineNumbers();
  if (persist) {
    void invoke("set_note_font_size", { fontSize: currentFontSize }).catch((err) =>
      console.warn("[shy-notes] set_note_font_size failed", err),
    );
  }
}

/** `*` = any run of characters; other regex metacharacters are literal. */
function wildcardToRegExp(pattern: string, matchCase: boolean): RegExp | null {
  if (!pattern) return null;
  const escaped = pattern
    .replace(/[.+?^${}()|[\]\\]/g, "\\$&")
    .replace(/\*/g, "[\\s\\S]*?");
  try {
    return new RegExp(escaped, matchCase ? "g" : "gi");
  } catch {
    return null;
  }
}

function matchCaseEnabled(): boolean {
  return !!(document.getElementById("find-match-case") as HTMLInputElement | null)?.checked;
}

function lineAtOffset(text: string, offset: number): number {
  let line = 1;
  for (let i = 0; i < offset && i < text.length; i++) {
    if (text.charCodeAt(i) === 10) line++;
  }
  return line;
}

function previewAt(text: string, start: number, end: number): string {
  const lineStart = text.lastIndexOf("\n", start - 1) + 1;
  let lineEnd = text.indexOf("\n", end);
  if (lineEnd < 0) lineEnd = text.length;
  const line = text.slice(lineStart, lineEnd).trim();
  return line.length > 72 ? `${line.slice(0, 72)}…` : line;
}

function collectMatches(pattern: string): TextMatch[] {
  const re = wildcardToRegExp(pattern, matchCaseEnabled());
  if (!re) return [];
  const text = editor().value;
  const out: TextMatch[] = [];
  let m: RegExpExecArray | null;
  re.lastIndex = 0;
  while ((m = re.exec(text)) !== null) {
    const start = m.index;
    const end = start + m[0].length;
    // Empty match (e.g. pattern "*") — advance to avoid infinite loop.
    if (end === start) {
      re.lastIndex = start + 1;
      continue;
    }
    out.push({
      start,
      end,
      line: lineAtOffset(text, start),
      preview: previewAt(text, start, end),
    });
    if (out.length >= 500) break;
  }
  return out;
}

function scrollSelectionIntoView(el: HTMLTextAreaElement) {
  const before = el.value.slice(0, el.selectionStart);
  const line = before.split("\n").length;
  const lh = parseFloat(getComputedStyle(el).lineHeight) || 18;
  const target = (line - 1) * lh - el.clientHeight * 0.35;
  el.scrollTop = Math.max(0, target);
}

function selectMatch(match: TextMatch) {
  const el = editor();
  el.focus();
  el.setSelectionRange(match.start, match.end);
  scrollSelectionIntoView(el);
}

function refreshFindStatus() {
  const n = findMatches.length;
  if (!findQuery().value) {
    findStatus().textContent = "";
    return;
  }
  if (n === 0) {
    findStatus().textContent = "No matches";
    return;
  }
  findStatus().textContent = `${findIndex + 1} of ${n}`;
}

function revealMatch(index: number) {
  if (findMatches.length === 0) {
    findIndex = -1;
    refreshFindStatus();
    return;
  }
  findIndex = ((index % findMatches.length) + findMatches.length) % findMatches.length;
  selectMatch(findMatches[findIndex]);
  refreshFindStatus();
}

function runFind(keepIndex: boolean) {
  const q = findQuery().value;
  const prevStart = keepIndex && findIndex >= 0 ? findMatches[findIndex]?.start : -1;
  findMatches = collectMatches(q);
  if (findMatches.length === 0) {
    findIndex = -1;
    refreshFindStatus();
    return;
  }
  if (keepIndex && prevStart >= 0) {
    const near = findMatches.findIndex((m) => m.start >= prevStart);
    findIndex = near >= 0 ? near : 0;
  } else {
    const caret = editor().selectionStart;
    const near = findMatches.findIndex((m) => m.start >= caret);
    findIndex = near >= 0 ? near : 0;
  }
  revealMatch(findIndex);
}

function showFindResults() {
  const box = findResults();
  box.replaceChildren();
  if (findMatches.length === 0) {
    box.classList.add("hidden");
    return;
  }
  box.classList.remove("hidden");
  const frag = document.createDocumentFragment();
  for (let i = 0; i < findMatches.length; i++) {
    const m = findMatches[i];
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "find-result";
    btn.role = "option";
    btn.dataset.index = String(i);
    btn.innerHTML = `<span class="ln">L${m.line}</span>${escapeHtml(m.preview)}`;
    frag.appendChild(btn);
  }
  box.appendChild(frag);
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function setFindTab(tab: "find" | "replace") {
  findPanel()
    .querySelectorAll<HTMLButtonElement>(".find-tab")
    .forEach((b) => b.classList.toggle("active", b.dataset.findTab === tab));
  document.getElementById("find-replace-row")!.classList.toggle("hidden", tab !== "replace");
}

function openFindPanel(tab: "find" | "replace" = "find") {
  const panel = findPanel();
  panel.classList.remove("hidden");
  setFindTab(tab);
  const el = editor();
  const selected = el.value.slice(el.selectionStart, el.selectionEnd);
  if (selected && !selected.includes("\n")) {
    findQuery().value = selected;
  }
  findQuery().focus();
  findQuery().select();
  if (findQuery().value) runFind(false);
}

function closeFindPanel() {
  findPanel().classList.add("hidden");
  findResults().classList.add("hidden");
  findResults().replaceChildren();
  findStatus().textContent = "";
}

function replaceCurrent() {
  if (findMatches.length === 0) runFind(false);
  if (findIndex < 0 || findIndex >= findMatches.length) return;
  const match = findMatches[findIndex];
  const el = editor();
  const replacement = findReplaceInput().value;
  el.focus();
  el.setSelectionRange(match.start, match.end);
  const ok = document.execCommand("insertText", false, replacement);
  if (!ok) {
    el.value =
      el.value.slice(0, match.start) + replacement + el.value.slice(match.end);
    el.setSelectionRange(match.start + replacement.length, match.start + replacement.length);
  }
  scheduleSave();
  updateDocStats();
  // Re-scan and jump to next occurrence after this replace.
  const nextCaret = match.start + replacement.length;
  findMatches = collectMatches(findQuery().value);
  const near = findMatches.findIndex((m) => m.start >= nextCaret);
  findIndex = near >= 0 ? near : findMatches.length > 0 ? 0 : -1;
  if (findIndex >= 0) revealMatch(findIndex);
  else refreshFindStatus();
  showFindResults();
}

function replaceAll() {
  const q = findQuery().value;
  const re = wildcardToRegExp(q, matchCaseEnabled());
  if (!re) return;
  const el = editor();
  const replacement = findReplaceInput().value;
  const before = el.value;
  // Rebuild without empty matches; literal replacement string.
  const next = before.replace(re, (m) => (m.length === 0 ? m : replacement));
  if (next === before) {
    findStatus().textContent = "No matches";
    return;
  }
  el.focus();
  el.select();
  const ok = document.execCommand("insertText", false, next);
  if (!ok) {
    el.value = next;
  }
  scheduleSave();
  updateDocStats();
  findMatches = collectMatches(q);
  findIndex = findMatches.length > 0 ? 0 : -1;
  refreshFindStatus();
  showFindResults();
  if (findIndex >= 0) revealMatch(findIndex);
}

function wireFindPanel() {
  const panel = findPanel();
  const dragHandle = panel.querySelector("[data-find-drag]") as HTMLElement | null;
  if (dragHandle) {
    let dragging = false;
    let startX = 0;
    let startY = 0;
    let origLeft = 0;
    let origTop = 0;
    dragHandle.addEventListener("mousedown", (ev) => {
      const t = ev.target as HTMLElement | null;
      if (t?.closest("button")) return;
      ev.preventDefault();
      ev.stopPropagation();
      const rect = panel.getBoundingClientRect();
      const parent = appEl().getBoundingClientRect();
      dragging = true;
      startX = ev.clientX;
      startY = ev.clientY;
      origLeft = rect.left - parent.left;
      origTop = rect.top - parent.top;
      panel.style.left = `${origLeft}px`;
      panel.style.top = `${origTop}px`;
      panel.style.right = "auto";
    });
    window.addEventListener("mousemove", (ev) => {
      if (!dragging) return;
      const parent = appEl().getBoundingClientRect();
      const nextLeft = origLeft + (ev.clientX - startX);
      const nextTop = origTop + (ev.clientY - startY);
      const maxLeft = Math.max(0, parent.width - panel.offsetWidth);
      const maxTop = Math.max(0, parent.height - panel.offsetHeight);
      panel.style.left = `${Math.min(maxLeft, Math.max(0, nextLeft))}px`;
      panel.style.top = `${Math.min(maxTop, Math.max(0, nextTop))}px`;
    });
    window.addEventListener("mouseup", () => {
      dragging = false;
    });
  }

  document.querySelectorAll<HTMLButtonElement>(".find-tab").forEach((btn) => {
    btn.addEventListener("click", () => {
      const tab = btn.dataset.findTab === "replace" ? "replace" : "find";
      setFindTab(tab);
    });
  });
  document.getElementById("find-close")?.addEventListener("click", closeFindPanel);
  document.getElementById("find-next")?.addEventListener("click", () => {
    if (findMatches.length === 0) runFind(false);
    else revealMatch(findIndex + 1);
  });
  document.getElementById("find-prev")?.addEventListener("click", () => {
    if (findMatches.length === 0) runFind(false);
    else revealMatch(findIndex - 1);
  });
  document.getElementById("find-all-btn")?.addEventListener("click", () => {
    runFind(true);
    showFindResults();
  });
  document.getElementById("find-match-case")?.addEventListener("change", () => {
    if (findQuery().value) runFind(true);
  });
  document.getElementById("find-replace-one")?.addEventListener("click", replaceCurrent);
  document.getElementById("find-replace-all")?.addEventListener("click", replaceAll);
  findQuery().addEventListener("keydown", (ev) => {
    if (ev.key === "Enter") {
      ev.preventDefault();
      if (ev.shiftKey) {
        if (findMatches.length === 0) runFind(false);
        else revealMatch(findIndex - 1);
      } else {
        if (findMatches.length === 0) runFind(false);
        else revealMatch(findIndex + 1);
      }
    }
  });
  findReplaceInput().addEventListener("keydown", (ev) => {
    if (ev.key === "Enter") {
      ev.preventDefault();
      replaceCurrent();
    }
  });
  findResults().addEventListener("click", (ev) => {
    const btn = (ev.target as HTMLElement | null)?.closest("button[data-index]") as
      | HTMLButtonElement
      | null;
    if (!btn?.dataset.index) return;
    revealMatch(Number(btn.dataset.index));
  });
}

const MAX_DROP_BYTES = 2 * 1024 * 1024;
const TEXT_FILE_RE =
  /\.(txt|md|markdown|csv|tsv|log|json|rs|ts|tsx|js|jsx|css|html|xml|yml|yaml|toml|ini|cfg|conf|sh|py|go|c|h|cpp|hpp|java|kt|swift|rb|php|sql|env|gitignore)$/i;

function insertAtCursor(text: string) {
  const el = editor();
  el.focus();
  const start = el.selectionStart;
  const end = el.selectionEnd;
  // Prefer insertText so the drop joins the native undo stack (Ctrl/Cmd+Z).
  el.setSelectionRange(start, end);
  const inserted = document.execCommand("insertText", false, text);
  if (!inserted || el.value.slice(start, start + text.length) !== text) {
    el.value = el.value.slice(0, start) + text + el.value.slice(end);
    const caret = start + text.length;
    el.setSelectionRange(caret, caret);
  }
  scheduleSave();
}

function looksLikeText(bytes: Uint8Array): boolean {
  const n = Math.min(bytes.length, 8192);
  let suspicious = 0;
  for (let i = 0; i < n; i++) {
    const b = bytes[i];
    if (b === 0) return false;
    if (b < 7 || (b > 13 && b < 32)) suspicious++;
  }
  return suspicious / Math.max(n, 1) < 0.05;
}

async function readDroppedFile(file: File): Promise<string | null> {
  if (file.size > MAX_DROP_BYTES) {
    console.warn("[shy-notes] drop skipped (too large):", file.name);
    return null;
  }
  const typeOk =
    !file.type ||
    file.type.startsWith("text/") ||
    file.type === "application/json" ||
    file.type === "application/xml" ||
    file.type === "application/javascript";
  const nameOk = TEXT_FILE_RE.test(file.name);
  if (!typeOk && !nameOk) {
    console.warn("[shy-notes] drop skipped (not text):", file.name, file.type);
    return null;
  }
  const buf = new Uint8Array(await file.arrayBuffer());
  if (!looksLikeText(buf)) {
    console.warn("[shy-notes] drop skipped (binary):", file.name);
    return null;
  }
  return new TextDecoder("utf-8", { fatal: false }).decode(buf);
}

async function textFromDataTransfer(dt: DataTransfer): Promise<string | null> {
  const files = Array.from(dt.files ?? []);
  if (files.length > 0) {
    const parts: string[] = [];
    for (const file of files) {
      const body = await readDroppedFile(file);
      if (body == null) continue;
      parts.push(files.length > 1 ? `--- ${file.name} ---\n${body}` : body);
    }
    return parts.length ? parts.join("\n\n") : null;
  }
  const plain = dt.getData("text/plain");
  if (plain) return plain;
  const html = dt.getData("text/html");
  if (html) {
    const tmp = document.createElement("div");
    tmp.innerHTML = html;
    const text = tmp.textContent ?? "";
    return text || null;
  }
  return null;
}

function wireDragAndDrop() {
  const root = appEl();
  let depth = 0;
  const uiLog = (msg: string) => {
    void invoke("frontend_log", { message: msg }).catch(() => {});
  };
  const setHover = (on: boolean) => {
    root.classList.toggle("drop-target", on);
    void invoke("set_drop_hover", { active: on }).catch((err) =>
      console.warn("[shy-notes] set_drop_hover failed", err),
    );
  };
  const clearHover = () => {
    depth = 0;
    setHover(false);
  };

  // Capture on window so dragover preventDefault always runs — otherwise macOS
  // never delivers drop (Tauri native DnD must stay disabled: dragDropEnabled false).
  const allowDrop = (ev: DragEvent) => {
    ev.preventDefault();
    ev.stopPropagation();
    if (ev.dataTransfer) ev.dataTransfer.dropEffect = "copy";
  };

  window.addEventListener("dragenter", (ev) => {
    allowDrop(ev);
    depth += 1;
    if (depth === 1) {
      setHover(true);
      uiLog("dragenter");
    }
  });
  window.addEventListener("dragover", allowDrop);
  window.addEventListener("dragleave", (ev) => {
    ev.preventDefault();
    depth = Math.max(0, depth - 1);
    if (depth === 0) {
      setHover(false);
      uiLog("dragleave");
    }
  });
  window.addEventListener("drop", (ev) => {
    allowDrop(ev);
    clearHover();
    const dt = ev.dataTransfer;
    if (!dt) {
      uiLog("drop without dataTransfer");
      return;
    }
    const types = Array.from(dt.types ?? []);
    uiLog(`drop types=[${types.join(",")}] files=${dt.files?.length ?? 0}`);
    void textFromDataTransfer(dt)
      .then((text) => {
        if (text == null || text === "") {
          uiLog("drop produced empty text");
          return;
        }
        uiLog(`drop insert chars=${text.length}`);
        insertAtCursor(text);
      })
      .catch((err) => {
        console.warn("[shy-notes] drop failed", err);
        uiLog(`drop failed: ${String(err)}`);
      });
  });
  window.addEventListener("blur", clearHover);
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

  wireDragAndDrop();
  wireFindPanel();

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

  editor().addEventListener("keydown", (ev) => {
    if (ev.key === "Tab" && !ev.altKey && !ev.ctrlKey && !ev.metaKey) {
      ev.preventDefault();
      insertAtCursor(" ".repeat(tabSize));
      return;
    }
  });

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
    } else if (action === "find") {
      openFindPanel("find");
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
    const mod = ev.metaKey || ev.ctrlKey;
    if (ev.key === "Escape") {
      closeMenu();
      setComboOpen(false);
      if (!findPanel().classList.contains("hidden")) {
        closeFindPanel();
        ev.preventDefault();
      }
      return;
    }
    if (mod && ev.key.toLowerCase() === "f" && !ev.shiftKey && !ev.altKey) {
      ev.preventDefault();
      openFindPanel("find");
      return;
    }
    if (mod && ev.altKey && ev.key.toLowerCase() === "f") {
      ev.preventDefault();
      openFindPanel("replace");
      return;
    }
    if (mod && (ev.key === "=" || ev.key === "+") && !ev.altKey) {
      ev.preventDefault();
      setFontSize(currentFontSize + 1, true);
      return;
    }
    if (mod && ev.key === "-" && !ev.altKey) {
      ev.preventDefault();
      setFontSize(currentFontSize - 1, true);
      return;
    }
    if (mod && ev.key.toLowerCase() === "g" && !findPanel().classList.contains("hidden")) {
      ev.preventDefault();
      if (findMatches.length === 0) runFind(false);
      else revealMatch(ev.shiftKey ? findIndex - 1 : findIndex + 1);
    }
  });

  closeBtn().addEventListener("click", () => {
    flushSave();
    void invoke("hide_window");
  });

  document.querySelector(".chrome")?.addEventListener("mousedown", (ev) => {
    const target = ev.target as HTMLElement | null;
    if (
      target?.closest("button") ||
      target?.closest(".app-menu") ||
      target?.closest("#note-combo")
    ) {
      return;
    }
    closeMenu();
    // Do not await before startDragging — on Windows the mouse capture is lost
    // after the first await and title-bar drag never starts (esp. when pinned).
    void invoke("begin_drag");
    void win
      .startDragging()
      .catch((err) => console.warn("[shy-notes] startDragging failed", err))
      .finally(() => {
        void invoke("end_drag");
      });
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

  await listen<{ trusted?: boolean; executable?: string }>("accessibility-needed", (ev) => {
    showA11y(ev.payload?.executable);
  });

  document.getElementById("a11y-request")?.addEventListener("click", () => {
    void invoke<{ trusted: boolean; executable: string }>("request_accessibility")
      .then((status) => {
        showA11y(status.executable);
        if (status.trusted) {
          a11y().classList.add("hidden");
        }
      })
      .catch((err) => console.warn("[shy-notes] request_accessibility failed", err));
  });
  document.getElementById("a11y-open")?.addEventListener("click", () => {
    void invoke("open_accessibility_settings");
  });
  document.getElementById("a11y-dismiss")?.addEventListener("click", () => {
    a11y().classList.add("hidden");
  });

  await win.onFocusChanged(async ({ payload: focused }) => {
    if (!focused) return;
    const status = await invoke<{ trusted: boolean; executable: string }>(
      "get_accessibility_status",
    );
    if (status.trusted) a11y().classList.add("hidden");
    else showA11y(status.executable);
  });

  {
    const status = await invoke<{ trusted: boolean; executable: string }>(
      "get_accessibility_status",
    );
    if (!status.trusted) showA11y(status.executable);
  }

  refreshLineNumbers();
  updateDocStats();
}

function showA11y(executable?: string) {
  a11y().classList.remove("hidden");
  const el = document.getElementById("a11y-exe");
  if (el && executable) {
    el.textContent = executable;
  }
}

void init();
