//! Local persistence for notes, pose, prefs, and metadata.

use interaction_core::{InteractionParams, WidgetPose};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

const STATE_VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteRecord {
    #[serde(flatten)]
    pub meta: NoteMeta,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteSummary {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub background: String,
    #[serde(default)]
    pub foreground: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStateSnapshot {
    pub version: u32,
    #[serde(default)]
    pub notes: Vec<NoteRecord>,
    #[serde(default)]
    pub active_note_id: String,
    pub pose: PoseDto,
    pub pinned: bool,
    #[serde(default)]
    pub prefs: UserPrefs,
    /// Legacy v1/v2 single-note fields — used only during migration.
    #[serde(default, skip_serializing)]
    text: String,
    #[serde(default, skip_serializing)]
    note: NoteMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoseDto {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteMeta {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default = "default_background")]
    pub background: String,
    /// Empty = theme default ink color.
    #[serde(default)]
    pub foreground: String,
    /// Editor font size in CSS pixels.
    #[serde(default = "default_font_size")]
    pub font_size: f64,
}

fn default_background() -> String {
    String::new() // empty = theme default
}

fn default_font_size() -> f64 {
    15.0
}

const INK_ON_LIGHT: &str = "#1c1a16";
const INK_ON_DARK: &str = "#ece7dc";
const SURFACE_FOR_DARK_INK: &str = "#f3efe6";
const SURFACE_FOR_LIGHT_INK: &str = "#1a1c1b";

fn parse_hex_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex.trim().trim_start_matches('#');
    let expand = |i: usize| -> Option<u8> {
        let c = h.as_bytes().get(i).copied()?;
        let s = format!("{}{}", c as char, c as char);
        u8::from_str_radix(&s, 16).ok()
    };
    match h.len() {
        3 => Some((expand(0)?, expand(1)?, expand(2)?)),
        6 => Some((
            u8::from_str_radix(&h[0..2], 16).ok()?,
            u8::from_str_radix(&h[2..4], 16).ok()?,
            u8::from_str_radix(&h[4..6], 16).ok()?,
        )),
        _ => None,
    }
}

fn relative_luminance(r: u8, g: u8, b: u8) -> f64 {
    fn channel(c: u8) -> f64 {
        let c = f64::from(c) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

fn is_light_color(hex: &str) -> Option<bool> {
    let (r, g, b) = parse_hex_rgb(hex)?;
    Some(relative_luminance(r, g, b) > 0.179)
}

/// Dark ink on light surfaces, light ink on dark surfaces.
pub fn contrasting_ink(background: &str) -> Option<String> {
    match is_light_color(background)? {
        true => Some(INK_ON_LIGHT.to_string()),
        false => Some(INK_ON_DARK.to_string()),
    }
}

/// Light paper under dark ink, dark paper under light ink.
pub fn contrasting_surface(foreground: &str) -> Option<String> {
    match is_light_color(foreground)? {
        true => Some(SURFACE_FOR_LIGHT_INK.to_string()),
        false => Some(SURFACE_FOR_DARK_INK.to_string()),
    }
}

impl Default for NoteMeta {
    fn default() -> Self {
        let now = now_iso();
        Self {
            id: Uuid::new_v4().to_string(),
            title: None,
            created_at: now.clone(),
            updated_at: now,
            background: default_background(),
            foreground: String::new(),
            font_size: default_font_size(),
        }
    }
}

impl NoteRecord {
    pub fn new_empty() -> Self {
        Self {
            meta: NoteMeta::default(),
            text: String::new(),
        }
    }

    pub fn label(&self) -> String {
        if let Some(title) = self
            .meta
            .title
            .as_ref()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
        {
            return title.to_string();
        }
        let preview = self
            .text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let preview = preview.trim();
        if preview.is_empty() {
            return "Untitled note".to_string();
        }
        let mut chars = preview.chars();
        let head: String = chars.by_ref().take(28).collect();
        if chars.next().is_some() {
            format!("{head}…")
        } else {
            head
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPrefs {
    pub repulsion_strength: f64,
    /// Influence / repulsion range in pixels (UI: "repulsion diameter").
    pub influence_radius: f64,
    pub capture_diameter: f64,
    pub show_glow: bool,
    pub show_line_numbers: bool,
    #[serde(default)]
    pub use_monospace: bool,
    #[serde(default)]
    pub open_at_startup: bool,
}

impl Default for UserPrefs {
    fn default() -> Self {
        let p = InteractionParams::default();
        Self {
            repulsion_strength: p.repulsion_strength,
            influence_radius: p.influence_radius,
            capture_diameter: p.capture_diameter,
            show_glow: true,
            show_line_numbers: false,
            use_monospace: false,
            open_at_startup: false,
        }
    }
}

impl UserPrefs {
    pub fn to_interaction_params(&self) -> InteractionParams {
        let mut p = InteractionParams::default();
        // Slider value is the peak force scale (1.0 ≈ gentle, 5.0 ≈ strong).
        p.repulsion_strength = self.repulsion_strength.clamp(0.2, 6.0);
        // Peak per-tick jump in px at closest approach (before distance falloff).
        p.repulsion_max_step = (self.repulsion_strength * 72.0).clamp(24.0, 400.0);
        p.influence_radius = self.influence_radius.clamp(80.0, 2400.0);
        p.capture_diameter = self.capture_diameter.clamp(40.0, 400.0);
        p
    }
}

impl Default for AppStateSnapshot {
    fn default() -> Self {
        let note = NoteRecord::new_empty();
        let id = note.meta.id.clone();
        Self {
            version: STATE_VERSION,
            notes: vec![note],
            active_note_id: id,
            pose: PoseDto {
                x: 80.0,
                y: 80.0,
                w: 320.0,
                h: 280.0,
            },
            pinned: false,
            prefs: UserPrefs::default(),
            text: String::new(),
            note: NoteMeta::default(),
        }
    }
}

impl AppStateSnapshot {
    pub fn to_pose(&self) -> WidgetPose {
        WidgetPose {
            x: self.pose.x,
            y: self.pose.y,
            w: self.pose.w,
            h: self.pose.h,
            pinned: self.pinned,
        }
    }

    pub fn ensure_active(&mut self) {
        if self.notes.is_empty() {
            let note = NoteRecord::new_empty();
            self.active_note_id = note.meta.id.clone();
            self.notes.push(note);
            return;
        }
        if !self
            .notes
            .iter()
            .any(|n| n.meta.id == self.active_note_id)
        {
            self.active_note_id = self.notes[0].meta.id.clone();
        }
    }

    pub fn active_index(&self) -> usize {
        self.notes
            .iter()
            .position(|n| n.meta.id == self.active_note_id)
            .unwrap_or(0)
    }

    pub fn active_note(&self) -> &NoteRecord {
        let i = self.active_index().min(self.notes.len().saturating_sub(1));
        &self.notes[i]
    }

    pub fn active_note_mut(&mut self) -> &mut NoteRecord {
        let i = self.active_index().min(self.notes.len().saturating_sub(1));
        &mut self.notes[i]
    }

    pub fn touch_updated(&mut self) {
        self.active_note_mut().meta.updated_at = now_iso();
    }

    pub fn summaries(&self) -> Vec<NoteSummary> {
        self.notes
            .iter()
            .map(|n| NoteSummary {
                id: n.meta.id.clone(),
                label: n.label(),
                background: n.meta.background.clone(),
                foreground: n.meta.foreground.clone(),
            })
            .collect()
    }

    pub fn create_note(&mut self) -> &NoteRecord {
        let note = NoteRecord::new_empty();
        self.active_note_id = note.meta.id.clone();
        self.notes.push(note);
        self.active_note()
    }

    pub fn switch_note(&mut self, id: &str) -> bool {
        if self.notes.iter().any(|n| n.meta.id == id) {
            self.active_note_id = id.to_string();
            true
        } else {
            false
        }
    }

    fn migrate(mut self) -> Self {
        if self.notes.is_empty() {
            let mut meta = self.note.clone();
            if meta.id.is_empty() {
                meta = NoteMeta::default();
            }
            self.active_note_id = meta.id.clone();
            self.notes.push(NoteRecord {
                meta,
                text: self.text.clone(),
            });
        }
        self.ensure_active();
        self.version = STATE_VERSION;
        self.text.clear();
        self
    }
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub fn state_path() -> PathBuf {
    let base = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("shy-notes");
    let _ = fs::create_dir_all(&base);
    base.join("state.json")
}

pub fn load_state() -> AppStateSnapshot {
    let path = state_path();
    match fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str::<AppStateSnapshot>(&raw)
            .unwrap_or_default()
            .migrate(),
        Err(_) => AppStateSnapshot::default(),
    }
}

pub fn save_state(state: &AppStateSnapshot) -> Result<(), String> {
    let path = state_path();
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(())
}
