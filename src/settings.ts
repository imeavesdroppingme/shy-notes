import { invoke } from "@tauri-apps/api/core";

type NoteMeta = {
  id: string;
  title?: string | null;
  created_at: string;
  updated_at: string;
  background: string;
  foreground: string;
  font_size: number;
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

const THEME_DEFAULT_BG = "#f3efe6";
const THEME_DEFAULT_FG = "#1c1a16";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

function bindRange(id: string, outId: string, format: (v: number) => string) {
  const input = $(id) as HTMLInputElement;
  const out = $(outId);
  const sync = () => {
    out.textContent = format(Number(input.value));
  };
  input.addEventListener("input", sync);
  sync();
  return input;
}

function formatStamp(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso || "—";
  return d.toLocaleString();
}

/** Peak px/tick at closest approach — mirrors native `strength * 72` mapping. */
function formatMaxStrength(v: number): string {
  const peak = Math.round(Math.min(400, Math.max(24, v * 72)));
  return `max ${v.toFixed(1)} (≈${peak} px)`;
}

function isHexColor(value: string): boolean {
  return /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(value);
}

function parseHexRgb(hex: string): [number, number, number] | null {
  const h = hex.trim().replace(/^#/, "");
  if (h.length === 3) {
    const r = parseInt(h[0] + h[0], 16);
    const g = parseInt(h[1] + h[1], 16);
    const b = parseInt(h[2] + h[2], 16);
    if ([r, g, b].some((n) => Number.isNaN(n))) return null;
    return [r, g, b];
  }
  if (h.length === 6) {
    const r = parseInt(h.slice(0, 2), 16);
    const g = parseInt(h.slice(2, 4), 16);
    const b = parseInt(h.slice(4, 6), 16);
    if ([r, g, b].some((n) => Number.isNaN(n))) return null;
    return [r, g, b];
  }
  return null;
}

function isLightColor(hex: string): boolean | null {
  const rgb = parseHexRgb(hex);
  if (!rgb) return null;
  const channel = (c: number) => {
    const x = c / 255;
    return x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
  };
  const L = 0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2]);
  return L > 0.179;
}

/** When only one custom color is set, pick a contrasting partner; never overwrite an existing custom. */
function pairNoteColors(background: string, foreground: string): { background: string; foreground: string } {
  let bg = background;
  let fg = foreground;
  if (bg && !fg) {
    const light = isLightColor(bg);
    if (light !== null) fg = light ? THEME_DEFAULT_FG : "#ece7dc";
  } else if (fg && !bg) {
    const light = isLightColor(fg);
    if (light !== null) bg = light ? "#1a1c1b" : THEME_DEFAULT_BG;
  }
  return { background: bg, foreground: fg };
}

function syncColorIndicator(
  input: HTMLInputElement,
  swatchId: string,
  labelId: string,
  usingDefault: boolean,
  fallback: string,
) {
  const swatch = $(swatchId);
  const label = $(labelId);
  const hex = usingDefault ? fallback : input.value;
  swatch.style.background = hex;
  swatch.classList.toggle("is-default", usingDefault);
  swatch.title = usingDefault ? "Theme default" : hex;
  label.textContent = usingDefault ? "Theme default" : hex.toLowerCase();
}

function setColorInput(input: HTMLInputElement, hex: string) {
  const normalized = hex.toLowerCase();
  input.value = normalized;
  input.setAttribute("value", normalized);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

function bindColorField(
  inputId: string,
  swatchId: string,
  labelId: string,
  resetId: string,
  fallback: string,
  initial: string,
) {
  const input = $(inputId) as HTMLInputElement;
  const hasCustom = isHexColor(initial);
  if (hasCustom) {
    delete input.dataset.reset;
    setColorInput(input, initial);
    syncColorIndicator(input, swatchId, labelId, false, fallback);
  } else {
    input.dataset.reset = "1";
    setColorInput(input, fallback);
    syncColorIndicator(input, swatchId, labelId, true, fallback);
  }

  const markCustom = () => {
    delete input.dataset.reset;
    syncColorIndicator(input, swatchId, labelId, false, fallback);
  };
  input.addEventListener("input", markCustom);
  input.addEventListener("change", markCustom);
  $(resetId).addEventListener("click", () => {
    input.dataset.reset = "1";
    setColorInput(input, fallback);
    syncColorIndicator(input, swatchId, labelId, true, fallback);
  });
  return input;
}

async function init() {
  const prefs = await invoke<UserPrefs>("get_prefs");
  const note = await invoke<NoteMeta>("get_note_meta");

  const strength = bindRange("repulsion-strength", "repulsion-strength-out", formatMaxStrength);
  const influence = bindRange("influence-radius", "influence-radius-out", (v) => `${Math.round(v)} px`);
  const capture = bindRange("capture-diameter", "capture-diameter-out", (v) => `${Math.round(v)} px`);
  const fontSize = bindRange("note-font-size", "note-font-size-out", (v) => `${Math.round(v)} px`);
  const glow = $("show-glow") as HTMLInputElement;
  const lines = $("show-line-numbers") as HTMLInputElement;
  const mono = $("use-monospace") as HTMLInputElement;
  const openAtStartup = $("open-at-startup") as HTMLInputElement;
  const title = $("note-title") as HTMLInputElement;

  strength.value = String(prefs.repulsion_strength);
  influence.value = String(prefs.influence_radius);
  capture.value = String(prefs.capture_diameter);
  fontSize.value = String(note.font_size || 15);
  glow.checked = prefs.show_glow;
  lines.checked = prefs.show_line_numbers;
  mono.checked = !!prefs.use_monospace;
  openAtStartup.checked = !!prefs.open_at_startup;
  strength.dispatchEvent(new Event("input"));
  influence.dispatchEvent(new Event("input"));
  capture.dispatchEvent(new Event("input"));
  fontSize.dispatchEvent(new Event("input"));

  title.value = note.title ?? "";
  const bg = bindColorField(
    "note-bg",
    "bg-swatch",
    "bg-label",
    "bg-reset",
    THEME_DEFAULT_BG,
    note.background ?? "",
  );
  const fg = bindColorField(
    "note-fg",
    "fg-swatch",
    "fg-label",
    "fg-reset",
    THEME_DEFAULT_FG,
    note.foreground ?? "",
  );

  $("meta-created").textContent = formatStamp(note.created_at);
  $("meta-updated").textContent = formatStamp(note.updated_at);
  $("meta-id").textContent = note.id;

  $("save").addEventListener("click", async () => {
    const btn = $("save") as HTMLButtonElement;
    if (btn.disabled) return;
    btn.disabled = true;
    try {
      const nextPrefs: UserPrefs = {
        repulsion_strength: Number(strength.value),
        influence_radius: Number(influence.value),
        capture_diameter: Number(capture.value),
        show_glow: glow.checked,
        show_line_numbers: lines.checked,
        use_monospace: mono.checked,
        open_at_startup: openAtStartup.checked,
      };
      const titleVal = title.value.trim();
      const paired = pairNoteColors(
        bg.dataset.reset === "1" ? "" : bg.value,
        fg.dataset.reset === "1" ? "" : fg.value,
      );
      await invoke("save_settings", {
        prefs: nextPrefs,
        note: {
          title: titleVal ? titleVal : null,
          background: paired.background,
          foreground: paired.foreground,
          fontSize: Number(fontSize.value),
        },
      });
      // Hide instead of destroy — avoids recreate races on rapid re-open.
      await invoke("hide_settings_window");
    } catch (err) {
      console.warn("[shy-notes] save settings failed", err);
      btn.disabled = false;
    }
  });
}

void init();
