mod mouse;
mod permissions;
mod persist;

use interaction_core::{
    DesktopLayout, InteractionCommand, InteractionController, MonitorInfo, WidgetPose,
};
use mouse::MouseTracker;
use parking_lot::Mutex;
use persist::{
    contrasting_ink, contrasting_surface, save_state, AppStateSnapshot, NoteMeta, NoteSummary,
    PoseDto, UserPrefs,
};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, RunEvent, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

fn debug_log(msg: impl AsRef<str>) {
    if std::env::var_os("SHY_NOTES_DEBUG").is_some() {
        eprintln!("[shy-notes] {}", msg.as_ref());
    }
}

struct VisibilityMenus {
    tray: MenuItem<tauri::Wry>,
    app: MenuItem<tauri::Wry>,
}

/// Filled during setup once native menus exist.
type MenuHandle = Arc<Mutex<Option<VisibilityMenus>>>;

fn sync_visibility_ui(app: &AppHandle, menus: &MenuHandle, concealed: bool) {
    let label = if concealed {
        "Show note"
    } else {
        "Hide note"
    };
    if let Some(m) = menus.lock().as_ref() {
        let _ = m.tray.set_text(label);
        let _ = m.app.set_text(label);
    }
    let _ = app.emit("visibility-changed", !concealed);
    debug_log(format!(
        "visibility -> {}",
        if concealed { "concealed" } else { "visible" }
    ));
}

#[derive(Debug, Clone, Serialize)]
struct InitialStateDto {
    version: u32,
    pose: PoseDto,
    pinned: bool,
    prefs: UserPrefs,
    notes: Vec<NoteSummary>,
    active_note_id: String,
    text: String,
    note: NoteMeta,
}

#[derive(Debug, Clone, Serialize)]
struct ActiveNoteDto {
    notes: Vec<NoteSummary>,
    active_note_id: String,
    text: String,
    note: NoteMeta,
}

fn dto_from_snapshot(snap: &AppStateSnapshot) -> InitialStateDto {
    let active = snap.active_note();
    InitialStateDto {
        version: snap.version,
        pose: snap.pose.clone(),
        pinned: snap.pinned,
        prefs: snap.prefs.clone(),
        notes: snap.summaries(),
        active_note_id: snap.active_note_id.clone(),
        text: active.text.clone(),
        note: active.meta.clone(),
    }
}

fn active_dto_from_snapshot(snap: &AppStateSnapshot) -> ActiveNoteDto {
    let active = snap.active_note();
    ActiveNoteDto {
        notes: snap.summaries(),
        active_note_id: snap.active_note_id.clone(),
        text: active.text.clone(),
        note: active.meta.clone(),
    }
}

fn emit_active_note(app: &AppHandle, snap: &AppStateSnapshot) {
    let _ = app.emit("active-note", active_dto_from_snapshot(snap));
}

fn apply_autostart(app: &AppHandle, enabled: bool) {
    // LaunchAgent I/O can stall the invoke thread on macOS — never block UI on it.
    let app = app.clone();
    std::thread::spawn(move || {
        use tauri_plugin_autostart::ManagerExt;
        let manager = app.autolaunch();
        let _ = if enabled {
            manager.enable()
        } else {
            manager.disable()
        };
    });
}

struct AppInner {
    controller: InteractionController,
    snapshot: AppStateSnapshot,
    permission_prompted: bool,
    /// Own hide tracking — macOS `is_visible` after `hide()` is unreliable for tray restore.
    main_concealed: bool,
}

type SharedState = Arc<Mutex<AppInner>>;

fn build_layout(window: &WebviewWindow) -> DesktopLayout {
    let mut monitors = Vec::new();
    if let Ok(list) = window.available_monitors() {
        for (i, m) in list.iter().enumerate() {
            let pos = m.position();
            let size = m.size();
            monitors.push(MonitorInfo {
                id: format!("m{i}"),
                x: pos.x as f64,
                y: pos.y as f64,
                w: size.width as f64,
                h: size.height as f64,
                scale: m.scale_factor(),
            });
        }
    }
    if monitors.is_empty() {
        monitors.push(MonitorInfo {
            id: "primary".into(),
            x: 0.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
            scale: 1.0,
        });
    }
    DesktopLayout { monitors }
}

fn apply_pose(window: &WebviewWindow, pose: &WidgetPose) {
    let _ = window.set_position(PhysicalPosition::new(pose.x as i32, pose.y as i32));
    let _ = window.set_size(PhysicalSize::new(pose.w as u32, pose.h as u32));
}

fn sync_pose_into_snapshot(inner: &mut AppInner) {
    inner.snapshot.pose = PoseDto {
        x: inner.controller.pose.x,
        y: inner.controller.pose.y,
        w: inner.controller.pose.w,
        h: inner.controller.pose.h,
    };
    inner.snapshot.pinned = inner.controller.pose.pinned;
}

fn persist_from_controller(inner: &mut AppInner) {
    sync_pose_into_snapshot(inner);
    persist_snapshot_async(inner.snapshot.clone());
}

fn persist_from_controller_blocking(inner: &mut AppInner) {
    sync_pose_into_snapshot(inner);
    let _ = save_state(&inner.snapshot);
}

fn persist_snapshot_async(snapshot: AppStateSnapshot) {
    std::thread::spawn(move || {
        let _ = save_state(&snapshot);
    });
}

fn apply_commands(
    window: &WebviewWindow,
    app: &AppHandle,
    cmds: &[InteractionCommand],
    show_glow: bool,
) {
    for cmd in cmds {
        match cmd {
            InteractionCommand::Noop | InteractionCommand::SuppressRepulsion => {}
            InteractionCommand::SetPose { x, y } => {
                let _ = window.set_position(PhysicalPosition::new(*x as i32, *y as i32));
            }
            InteractionCommand::SetVisual { pre_capture, glow } => {
                let glow = if show_glow { *glow } else { 0.0 };
                let _ = app.emit(
                    "visual-hints",
                    serde_json::json!({
                        "preCapture": *pre_capture && show_glow,
                        "glow": glow
                    }),
                );
            }
            InteractionCommand::RequestFocus => {
                if !window.is_focused().unwrap_or(false) {
                    let _ = window.set_focus();
                }
                let _ = app.emit("request-focus", ());
            }
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Suppresses rapid hide/show flip-flops from tray Click+DoubleClick pairs.
static VISIBILITY_GUARD_MS: AtomicU64 = AtomicU64::new(0);

fn visibility_guard_ok() -> bool {
    let now = now_ms();
    let prev = VISIBILITY_GUARD_MS.load(Ordering::SeqCst);
    if now.saturating_sub(prev) < 400 {
        debug_log("visibility event ignored (debounce)");
        return false;
    }
    VISIBILITY_GUARD_MS.store(now, Ordering::SeqCst);
    true
}

fn show_main_window(app: &AppHandle, state: &SharedState, menus: &MenuHandle) {
    let pose = {
        let mut inner = state.lock();
        inner.main_concealed = false;
        inner.controller.pose
    };
    sync_visibility_ui(app, menus, false);
    debug_log("show_main_window");

    #[cfg(target_os = "macos")]
    {
        let _ = app.show();
    }
    if let Some(window) = app.get_webview_window("main") {
        apply_pose(&window, &pose);
        let _ = window.unminimize();
        let _ = window.set_always_on_top(false);
        let _ = window.show();
        let _ = window.set_always_on_top(true);
        let _ = window.set_focus();
    }
}

fn hide_main_window(app: &AppHandle, state: &SharedState, menus: &MenuHandle) {
    state.lock().main_concealed = true;
    sync_visibility_ui(app, menus, true);
    debug_log("hide_main_window");
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_always_on_top(false);
        let _ = window.hide();
    }
}

fn toggle_visibility(app: &AppHandle, state: &SharedState, menus: &MenuHandle) {
    if !visibility_guard_ok() {
        return;
    }
    let concealed = state.lock().main_concealed;
    if concealed {
        show_main_window(app, state, menus);
    } else {
        hide_main_window(app, state, menus);
    }
}

/// Tray left-click: only reveal. Hide via × / menu / hotkey — avoids Click+DoubleClick flash.
fn tray_reveal(app: &AppHandle, state: &SharedState, menus: &MenuHandle) {
    if !visibility_guard_ok() {
        return;
    }
    if state.lock().main_concealed {
        show_main_window(app, state, menus);
    } else {
        // Already visible: just raise/focus.
        debug_log("tray click → focus existing window");
        #[cfg(target_os = "macos")]
        {
            let _ = app.show();
        }
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_always_on_top(true);
            let _ = window.set_focus();
        }
    }
}

fn reset_position(app: &AppHandle, state: &SharedState) {
    if let Some(window) = app.get_webview_window("main") {
        let layout = build_layout(&window);
        let mut inner = state.lock();
        inner.controller.set_layout(layout);
        let mut pose = inner.controller.pose;
        pose.x = 80.0;
        pose.y = 80.0;
        pose = interaction_core::clamp_pose_to_layout(&pose, &inner.controller.layout);
        inner.controller.pose = pose;
        apply_pose(&window, &pose);
        persist_from_controller(&mut inner);
    }
}

#[tauri::command]
fn reset_note_position(app: AppHandle, state: tauri::State<'_, SharedState>) {
    reset_position(&app, &state);
}

fn open_settings_window(app: &AppHandle) -> Result<(), String> {
    // Serialize create/focus so double-clicks cannot build two "settings" labels.
    static GATE: Mutex<()> = Mutex::new(());
    let _gate = GATE.lock();

    if let Some(existing) = app.get_webview_window("settings") {
        let was_visible = existing.is_visible().unwrap_or(false);
        let _ = existing.set_always_on_top(true);
        position_beside_note(app, &existing);
        let _ = existing.unminimize();
        let _ = existing.show();
        let _ = existing.set_focus();
        // Refresh form after a prior Save→hide (or prefs changed while closed).
        if !was_visible {
            let _ = existing.eval("window.location.reload()");
        }
        return Ok(());
    }
    let settings = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("Shy notes — Settings")
        .inner_size(440.0, 560.0)
        .min_inner_size(360.0, 420.0)
        .resizable(true)
        .always_on_top(true)
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;
    position_beside_note(app, &settings);
    let _ = settings.show();
    let _ = settings.set_focus();
    Ok(())
}

fn open_about_window(app: &AppHandle) -> Result<(), String> {
    static GATE: Mutex<()> = Mutex::new(());
    let _gate = GATE.lock();

    if let Some(existing) = app.get_webview_window("about") {
        let _ = existing.set_always_on_top(true);
        position_beside_note(app, &existing);
        let _ = existing.unminimize();
        let _ = existing.show();
        let _ = existing.set_focus();
        return Ok(());
    }
    let about = WebviewWindowBuilder::new(app, "about", WebviewUrl::App("about.html".into()))
        .title("Shy notes — About")
        .inner_size(380.0, 320.0)
        .resizable(false)
        .always_on_top(true)
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;
    position_beside_note(app, &about);
    let _ = about.show();
    let _ = about.set_focus();
    Ok(())
}

/// Windows deadlocks if a secondary WebView is created on the UI/command thread.
fn spawn_open_settings(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = open_settings_window(&app) {
            debug_log(format!("open_settings failed: {e}"));
        }
    });
}

fn spawn_open_about(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = open_about_window(&app) {
            debug_log(format!("open_about failed: {e}"));
        }
    });
}

/// True when Settings or About is visible — note should stay put.
fn overlay_blocks_repulsion(app: &AppHandle) -> bool {
    for label in ["settings", "about"] {
        if let Some(w) = app.get_webview_window(label) {
            if w.is_visible().unwrap_or(false) {
                return true;
            }
        }
    }
    false
}

fn position_beside_note(app: &AppHandle, window: &WebviewWindow) {
    let Some(main) = app.get_webview_window("main") else {
        return;
    };
    let Ok(pos) = main.outer_position() else {
        return;
    };
    let Ok(size) = main.outer_size() else {
        return;
    };
    let Ok(s_size) = window.outer_size() else {
        return;
    };
    let gap = 20i32;
    let mut x = pos.x + size.width as i32 + gap;
    let mut y = pos.y;

    if let Ok(Some(mon)) = main.current_monitor() {
        let mpos = mon.position();
        let msize = mon.size();
        let right_limit = mpos.x + msize.width as i32 - s_size.width as i32 - 12;
        if x > right_limit {
            x = pos.x - s_size.width as i32 - gap;
        }
        if x < mpos.x + 8 {
            x = mpos.x + 8;
        }
        let bottom_limit = mpos.y + msize.height as i32 - s_size.height as i32 - 12;
        if y > bottom_limit {
            y = bottom_limit.max(mpos.y + 8);
        }
        if y < mpos.y + 8 {
            y = mpos.y + 8;
        }
    }

    let _ = window.set_position(PhysicalPosition::new(x, y));
}

#[tauri::command]
fn get_initial_state(state: tauri::State<'_, SharedState>) -> InitialStateDto {
    let mut inner = state.lock();
    inner.snapshot.ensure_active();
    dto_from_snapshot(&inner.snapshot)
}

#[tauri::command]
fn save_text(text: String, state: tauri::State<'_, SharedState>, app: AppHandle) -> Result<(), String> {
    let (snapshot, label_changed) = {
        let mut inner = state.lock();
        inner.snapshot.ensure_active();
        let note = inner.snapshot.active_note_mut();
        if note.text == text {
            return Ok(());
        }
        let old_label = note.label();
        note.text = text;
        // Throttle metadata churn: bump updated_at at most every ~5s while typing.
        let should_touch = match chrono::DateTime::parse_from_rfc3339(&note.meta.updated_at) {
            Ok(prev) => {
                let prev_utc = prev.with_timezone(&chrono::Utc);
                (chrono::Utc::now() - prev_utc).num_seconds() >= 5
            }
            Err(_) => true,
        };
        if should_touch {
            inner.snapshot.touch_updated();
        }
        let new_label = inner.snapshot.active_note().label();
        (inner.snapshot.clone(), old_label != new_label)
    };
    persist_snapshot_async(snapshot.clone());
    if label_changed {
        emit_active_note(&app, &snapshot);
    }
    Ok(())
}

#[tauri::command]
fn list_notes(state: tauri::State<'_, SharedState>) -> Vec<NoteSummary> {
    state.lock().snapshot.summaries()
}

#[tauri::command]
fn switch_note(
    id: String,
    state: tauri::State<'_, SharedState>,
    app: AppHandle,
) -> Result<ActiveNoteDto, String> {
    let dto = {
        let mut inner = state.lock();
        if !inner.snapshot.switch_note(&id) {
            return Err("note not found".into());
        }
        persist_from_controller(&mut inner);
        active_dto_from_snapshot(&inner.snapshot)
    };
    let _ = app.emit("active-note", &dto);
    Ok(dto)
}

#[tauri::command]
fn create_note(state: tauri::State<'_, SharedState>, app: AppHandle) -> Result<ActiveNoteDto, String> {
    let dto = {
        let mut inner = state.lock();
        inner.snapshot.create_note();
        persist_from_controller(&mut inner);
        active_dto_from_snapshot(&inner.snapshot)
    };
    let _ = app.emit("active-note", &dto);
    Ok(dto)
}

#[tauri::command]
fn hide_window(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    menus: tauri::State<'_, MenuHandle>,
) {
    hide_main_window(&app, &state, &menus);
}

#[tauri::command]
fn show_window(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    menus: tauri::State<'_, MenuHandle>,
) {
    show_main_window(&app, &state, &menus);
}

#[tauri::command]
fn toggle_window_visibility(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    menus: tauri::State<'_, MenuHandle>,
) {
    toggle_visibility(&app, &state, &menus);
}

#[tauri::command]
fn set_pinned(pinned: bool, state: tauri::State<'_, SharedState>, app: AppHandle) -> bool {
    debug_log(format!("set_pinned({pinned}) enter"));
    let started = std::time::Instant::now();
    let snapshot = {
        let mut inner = state.lock();
        debug_log(format!(
            "set_pinned got lock in {}ms",
            started.elapsed().as_millis()
        ));
        inner.controller.set_pinned(pinned);
        inner.snapshot.pinned = pinned;
        sync_pose_into_snapshot(&mut inner);
        inner.snapshot.clone()
    };
    debug_log(format!(
        "set_pinned released lock after {}ms",
        started.elapsed().as_millis()
    ));
    persist_snapshot_async(snapshot);
    let _ = app.emit("pinned-changed", pinned);
    debug_log("set_pinned done");
    pinned
}

#[tauri::command]
fn get_pinned(state: tauri::State<'_, SharedState>) -> bool {
    state.lock().controller.pose.pinned
}

#[tauri::command]
fn begin_drag(state: tauri::State<'_, SharedState>) {
    state.lock().controller.begin_drag();
}

#[tauri::command]
fn end_drag(state: tauri::State<'_, SharedState>, app: AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let mut inner = state.lock();
        if let Ok(pos) = window.outer_position() {
            inner.controller.end_drag(pos.x as f64, pos.y as f64);
        }
        persist_from_controller(&mut inner);
    }
}

#[tauri::command]
fn notify_resized(w: f64, h: f64, state: tauri::State<'_, SharedState>) {
    let mut inner = state.lock();
    inner.controller.set_size(w, h);
    persist_from_controller(&mut inner);
}

#[tauri::command]
fn check_accessibility() -> bool {
    permissions::is_accessibility_trusted(false)
}

#[tauri::command]
fn open_accessibility_settings() {
    permissions::open_accessibility_settings();
}

#[tauri::command]
fn quit_app(app: AppHandle, state: tauri::State<'_, SharedState>) {
    {
        let mut inner = state.lock();
        persist_from_controller_blocking(&mut inner);
    }
    app.exit(0);
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NoteSettingsPatch {
    title: Option<String>,
    background: String,
    foreground: String,
    font_size: f64,
}

#[tauri::command]
fn save_settings(
    prefs: UserPrefs,
    note: NoteSettingsPatch,
    state: tauri::State<'_, SharedState>,
    app: AppHandle,
) -> Result<(), String> {
    let (saved_prefs, snap) = {
        let mut inner = state.lock();
        inner.snapshot.prefs = prefs;
        inner.controller.params = inner.snapshot.prefs.to_interaction_params();

        let meta = &mut inner.snapshot.active_note_mut().meta;
        meta.title = note
            .title
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        let bg = note.background.trim().to_string();
        let fg = note.foreground.trim().to_string();
        meta.background = bg.clone();
        meta.foreground = fg.clone();
        if !bg.is_empty() && meta.foreground.is_empty() {
            if let Some(ink) = contrasting_ink(&bg) {
                meta.foreground = ink;
            }
        } else if !fg.is_empty() && meta.background.is_empty() {
            if let Some(surface) = contrasting_surface(&fg) {
                meta.background = surface;
            }
        }
        meta.font_size = note.font_size.clamp(10.0, 28.0);
        inner.snapshot.touch_updated();
        persist_from_controller(&mut inner);
        (
            inner.snapshot.prefs.clone(),
            inner.snapshot.clone(),
        )
    };
    apply_autostart(&app, saved_prefs.open_at_startup);
    let _ = app.emit("prefs-changed", &saved_prefs);
    emit_active_note(&app, &snap);
    Ok(())
}

#[tauri::command]
fn hide_settings_window(app: AppHandle) -> Result<(), String> {
    if let Some(settings) = app.get_webview_window("settings") {
        let _ = settings.hide();
    }
    Ok(())
}

#[tauri::command]
async fn open_settings(app: AppHandle) -> Result<(), String> {
    // Must be async on Windows — sync WebviewWindowBuilder::build deadlocks WebView2.
    open_settings_window(&app)
}

#[tauri::command]
async fn open_about(app: AppHandle) -> Result<(), String> {
    open_about_window(&app)
}

#[tauri::command]
fn get_prefs(state: tauri::State<'_, SharedState>) -> UserPrefs {
    state.lock().snapshot.prefs.clone()
}

#[tauri::command]
fn set_prefs(
    prefs: UserPrefs,
    state: tauri::State<'_, SharedState>,
    app: AppHandle,
) -> Result<UserPrefs, String> {
    let saved = {
        let mut inner = state.lock();
        inner.snapshot.prefs = prefs;
        inner.controller.params = inner.snapshot.prefs.to_interaction_params();
        persist_from_controller(&mut inner);
        inner.snapshot.prefs.clone()
    };
    apply_autostart(&app, saved.open_at_startup);
    let _ = app.emit("prefs-changed", &saved);
    Ok(saved)
}

#[tauri::command]
fn get_note_meta(state: tauri::State<'_, SharedState>) -> NoteMeta {
    state.lock().snapshot.active_note().meta.clone()
}

#[tauri::command]
fn set_note_title(
    title: Option<String>,
    state: tauri::State<'_, SharedState>,
    app: AppHandle,
) -> Result<NoteMeta, String> {
    let (note, snap) = {
        let mut inner = state.lock();
        let cleaned = title
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        inner.snapshot.active_note_mut().meta.title = cleaned;
        inner.snapshot.touch_updated();
        persist_from_controller(&mut inner);
        (
            inner.snapshot.active_note().meta.clone(),
            inner.snapshot.clone(),
        )
    };
    emit_active_note(&app, &snap);
    Ok(note)
}

#[tauri::command]
fn set_note_background(
    background: String,
    state: tauri::State<'_, SharedState>,
    app: AppHandle,
) -> Result<NoteMeta, String> {
    let (note, snap) = {
        let mut inner = state.lock();
        let meta = &mut inner.snapshot.active_note_mut().meta;
        let bg = background.trim().to_string();
        meta.background = bg.clone();
        // Auto-pair ink only when the user has not chosen a custom foreground yet.
        if !bg.is_empty() && meta.foreground.trim().is_empty() {
            if let Some(ink) = contrasting_ink(&bg) {
                meta.foreground = ink;
            }
        }
        inner.snapshot.touch_updated();
        persist_from_controller(&mut inner);
        (
            inner.snapshot.active_note().meta.clone(),
            inner.snapshot.clone(),
        )
    };
    emit_active_note(&app, &snap);
    Ok(note)
}

#[tauri::command]
fn set_note_foreground(
    foreground: String,
    state: tauri::State<'_, SharedState>,
    app: AppHandle,
) -> Result<NoteMeta, String> {
    let (note, snap) = {
        let mut inner = state.lock();
        let meta = &mut inner.snapshot.active_note_mut().meta;
        let fg = foreground.trim().to_string();
        meta.foreground = fg.clone();
        // Auto-pair surface only when the user has not chosen a custom background yet.
        if !fg.is_empty() && meta.background.trim().is_empty() {
            if let Some(surface) = contrasting_surface(&fg) {
                meta.background = surface;
            }
        }
        inner.snapshot.touch_updated();
        persist_from_controller(&mut inner);
        (
            inner.snapshot.active_note().meta.clone(),
            inner.snapshot.clone(),
        )
    };
    emit_active_note(&app, &snap);
    Ok(note)
}

#[tauri::command]
fn set_note_font_size(
    font_size: f64,
    state: tauri::State<'_, SharedState>,
    app: AppHandle,
) -> Result<NoteMeta, String> {
    let (note, snap) = {
        let mut inner = state.lock();
        inner.snapshot.active_note_mut().meta.font_size = font_size.clamp(10.0, 28.0);
        inner.snapshot.touch_updated();
        persist_from_controller(&mut inner);
        (
            inner.snapshot.active_note().meta.clone(),
            inner.snapshot.clone(),
        )
    };
    emit_active_note(&app, &snap);
    Ok(note)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let snapshot = persist::load_state();
    let pose = snapshot.to_pose();
    let params = snapshot.prefs.to_interaction_params();
    let controller =
        InteractionController::new(pose, DesktopLayout::default(), params);

    let shared: SharedState = Arc::new(Mutex::new(AppInner {
        controller,
        snapshot,
        permission_prompted: false,
        main_concealed: false,
    }));

    let shared_for_setup = shared.clone();
    let menus: MenuHandle = Arc::new(Mutex::new(None));
    let menus_for_setup = menus.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(shared.clone())
        .manage(menus.clone())
        .invoke_handler(tauri::generate_handler![
            get_initial_state,
            save_text,
            set_pinned,
            get_pinned,
            begin_drag,
            end_drag,
            notify_resized,
            check_accessibility,
            open_accessibility_settings,
            quit_app,
            hide_window,
            show_window,
            toggle_window_visibility,
            open_settings,
            open_about,
            hide_settings_window,
            save_settings,
            get_prefs,
            set_prefs,
            get_note_meta,
            set_note_title,
            set_note_background,
            set_note_foreground,
            set_note_font_size,
            reset_note_position,
            list_notes,
            switch_note,
            create_note,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let window = app
                .get_webview_window("main")
                .expect("main window missing");
            let _ = window.set_title("Shy notes");

            {
                let mut inner = shared_for_setup.lock();
                inner.snapshot.ensure_active();
                let layout = build_layout(&window);
                inner.controller.set_layout(layout);
                let pose = inner.controller.pose;
                apply_pose(&window, &pose);
                let _ = window.set_always_on_top(true);
                apply_autostart(&handle, inner.snapshot.prefs.open_at_startup);
            }

            let win_close = handle.clone();
            let state_close = shared_for_setup.clone();
            let menus_close = menus_for_setup.clone();
            window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    hide_main_window(&win_close, &state_close, &menus_close);
                }
            });

            let pin_i = MenuItem::with_id(app, "pin", "Pin", true, None::<&str>)?;
            let visibility_i =
                MenuItem::with_id(app, "visibility", "Hide note", true, None::<&str>)?;
            let settings_i = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
            let about_i = MenuItem::with_id(app, "about", "About Shy notes", true, None::<&str>)?;
            let reset_i = MenuItem::with_id(app, "reset", "Reset position", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let tray_sep = PredefinedMenuItem::separator(app)?;
            let tray_menu = Menu::with_items(
                app,
                &[
                    &visibility_i,
                    &settings_i,
                    &pin_i,
                    &reset_i,
                    &tray_sep,
                    &about_i,
                    &quit_i,
                ],
            )?;

            let app_settings =
                MenuItem::with_id(app, "app-settings", "Settings…", true, None::<&str>)?;
            let app_about =
                MenuItem::with_id(app, "app-about", "About Shy notes", true, None::<&str>)?;
            let app_visibility =
                MenuItem::with_id(app, "app-visibility", "Hide note", true, None::<&str>)?;
            let app_pin = MenuItem::with_id(app, "app-pin", "Pin", true, None::<&str>)?;
            let app_reset =
                MenuItem::with_id(app, "app-reset", "Reset position", true, None::<&str>)?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let app_quit = PredefinedMenuItem::quit(app, Some("Quit Shy notes"))?;
            let app_submenu = Submenu::with_items(
                app,
                "Shy notes",
                true,
                &[
                    &app_about,
                    &app_settings,
                    &sep1,
                    &app_visibility,
                    &app_pin,
                    &app_reset,
                    &sep2,
                    &app_quit,
                ],
            )?;
            // Native Edit menu wires Cmd/Ctrl+C/V/X/A (and Undo/Redo) into the webview.
            let edit_undo = PredefinedMenuItem::undo(app, None)?;
            let edit_redo = PredefinedMenuItem::redo(app, None)?;
            let edit_cut = PredefinedMenuItem::cut(app, None)?;
            let edit_copy = PredefinedMenuItem::copy(app, None)?;
            let edit_paste = PredefinedMenuItem::paste(app, None)?;
            let edit_select_all = PredefinedMenuItem::select_all(app, None)?;
            let edit_sep1 = PredefinedMenuItem::separator(app)?;
            let edit_sep2 = PredefinedMenuItem::separator(app)?;
            let edit_menu = Submenu::with_items(
                app,
                "Edit",
                true,
                &[
                    &edit_undo,
                    &edit_redo,
                    &edit_sep1,
                    &edit_cut,
                    &edit_copy,
                    &edit_paste,
                    &edit_sep2,
                    &edit_select_all,
                ],
            )?;
            let app_menu = Menu::with_items(app, &[&app_submenu, &edit_menu])?;
            app.set_menu(app_menu)?;

            *menus_for_setup.lock() = Some(VisibilityMenus {
                tray: visibility_i,
                app: app_visibility,
            });

            let state_menu = shared_for_setup.clone();
            let handle_menu = handle.clone();
            let menus_menu = menus_for_setup.clone();
            let on_menu = Arc::new(move |app: &AppHandle, id: &str| match id {
                "pin" | "app-pin" => {
                    debug_log("menu pin toggle");
                    let (pinned, snapshot) = {
                        let mut inner = state_menu.lock();
                        let next = !inner.controller.pose.pinned;
                        inner.controller.set_pinned(next);
                        inner.snapshot.pinned = next;
                        sync_pose_into_snapshot(&mut inner);
                        (next, inner.snapshot.clone())
                    };
                    persist_snapshot_async(snapshot);
                    let _ = handle_menu.emit("pinned-changed", pinned);
                }
                "visibility" | "app-visibility" => {
                    toggle_visibility(&handle_menu, &state_menu, &menus_menu);
                }
                "settings" | "app-settings" => {
                    spawn_open_settings(app);
                }
                "about" | "app-about" => {
                    spawn_open_about(app);
                }
                "reset" | "app-reset" => reset_position(&handle_menu, &state_menu),
                "quit" => {
                    {
                        let mut inner = state_menu.lock();
                        persist_from_controller_blocking(&mut inner);
                    }
                    handle_menu.exit(0);
                }
                _ => {}
            });

            let on_menu_tray = on_menu.clone();
            let tray_icon = app
                .default_window_icon()
                .cloned()
                .ok_or_else(|| "missing default window icon".to_string())?;
            let state_tray = shared_for_setup.clone();
            let menus_tray = menus_for_setup.clone();
            let _tray = TrayIconBuilder::new()
                .menu(&tray_menu)
                .tooltip("Shy notes")
                .icon(tray_icon)
                .icon_as_template(false)
                // Left click toggles visibility; menu on right click.
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| {
                    debug_log(format!("tray menu {}", event.id.as_ref()));
                    on_menu_tray(app, event.id.as_ref());
                })
                .on_tray_icon_event(move |tray, event| {
                    match event {
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                        | TrayIconEvent::DoubleClick {
                            button: MouseButton::Left,
                            ..
                        } => {
                            debug_log("tray left click → reveal");
                            tray_reveal(tray.app_handle(), &state_tray, &menus_tray);
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            let on_menu_bar = on_menu.clone();
            app.on_menu_event(move |app, event| {
                on_menu_bar(app, event.id.as_ref());
            });

            let shortcut = if cfg!(target_os = "macos") {
                Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::Space)
            } else {
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space)
            };
            let handle_gs = handle.clone();
            let state_gs = shared_for_setup.clone();
            let menus_gs = menus_for_setup.clone();
            app.global_shortcut().on_shortcut(shortcut, move |_app, _sc, event| {
                if event.state == ShortcutState::Pressed {
                    toggle_visibility(&handle_gs, &state_gs, &menus_gs);
                }
            })?;

            let loop_state = shared_for_setup.clone();
            let loop_handle = handle.clone();
            std::thread::spawn(move || {
                // Keep DeviceState off SharedState: on Linux it is !Send (Rc/X11).
                let mouse = MouseTracker::new();
                loop {
                    std::thread::sleep(Duration::from_millis(16));
                    let Some(window) = loop_handle.get_webview_window("main") else {
                        continue;
                    };

                    #[cfg(target_os = "macos")]
                    {
                        let trusted = permissions::is_accessibility_trusted(false);
                        if !trusted {
                            let mut inner = loop_state.lock();
                            if !inner.permission_prompted {
                                inner.permission_prompted = true;
                                let _ = loop_handle.emit("accessibility-needed", ());
                            }
                            continue;
                        }
                    }

                    // Window APIs must stay outside the state lock. Holding the mutex
                    // while calling into AppKit/Win32 deadlocks when a UI command waits
                    // on the same lock (pin, settings, save).
                    let layout = build_layout(&window);
                    let scale = window.scale_factor().unwrap_or(1.0);
                    let outer = match (window.outer_position(), window.outer_size()) {
                        (Ok(pos), Ok(size)) => Some((pos, size)),
                        _ => None,
                    };
                    let block_repulsion = overlay_blocks_repulsion(&loop_handle);
                    let ctrl = mouse.ctrl_held();
                    let mut sample = mouse.sample();
                    sample.x_phys *= scale;
                    sample.y_phys *= scale;
                    let (cmds, show_glow) = {
                        let mut inner = loop_state.lock();
                        inner.controller.set_layout(layout);
                        if let Some((pos, size)) = outer {
                            inner.controller.sync_outer_pose(
                                pos.x as f64,
                                pos.y as f64,
                                size.width as f64,
                                size.height as f64,
                            );
                        }
                        inner.controller.suppress_repulsion = ctrl || block_repulsion;
                        let show_glow = inner.snapshot.prefs.show_glow;
                        (inner.controller.on_mouse(sample), show_glow)
                    };
                    apply_commands(&window, &loop_handle, &cmds, show_glow);
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building shy-notes")
        .run(|app_handle, event| {
            if let RunEvent::ExitRequested { .. } | RunEvent::Exit = event {
                if let Some(state) = app_handle.try_state::<SharedState>() {
                    let mut inner = state.lock();
                    persist_from_controller_blocking(&mut inner);
                }
            }
        });
}
