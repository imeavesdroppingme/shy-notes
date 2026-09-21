mod mouse;
mod permissions;
mod persist;

use interaction_core::{
    DesktopLayout, InteractionCommand, InteractionController, InteractionParams, MonitorInfo,
    WidgetPose,
};
use mouse::MouseTracker;
use parking_lot::Mutex;
use persist::{save_state, AppStateSnapshot, PoseDto};
use std::sync::Arc;
use std::time::Duration;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, RunEvent, WebviewWindow,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

struct AppInner {
    controller: InteractionController,
    snapshot: AppStateSnapshot,
    mouse: MouseTracker,
    permission_prompted: bool,
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

fn persist_from_controller(inner: &AppInner) {
    let mut snap = inner.snapshot.clone();
    snap.pose = PoseDto {
        x: inner.controller.pose.x,
        y: inner.controller.pose.y,
        w: inner.controller.pose.w,
        h: inner.controller.pose.h,
    };
    snap.pinned = inner.controller.pose.pinned;
    let _ = save_state(&snap);
}

fn apply_commands(window: &WebviewWindow, app: &AppHandle, cmds: &[InteractionCommand]) {
    for cmd in cmds {
        match cmd {
            InteractionCommand::Noop | InteractionCommand::SuppressRepulsion => {}
            InteractionCommand::SetPose { x, y } => {
                let _ = window.set_position(PhysicalPosition::new(*x as i32, *y as i32));
            }
            InteractionCommand::SetVisual { pre_capture } => {
                let _ = app.emit("visual-hints", serde_json::json!({ "preCapture": pre_capture }));
            }
            InteractionCommand::RequestFocus => {
                // Focus once; avoid re-focus storms that interrupt typing.
                if !window.is_focused().unwrap_or(false) {
                    let _ = window.set_focus();
                }
                let _ = app.emit("request-focus", ());
            }
        }
    }
}

fn toggle_visibility(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(true) {
            let _ = window.hide();
        } else {
            let _ = window.show();
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
        persist_from_controller(&inner);
    }
}

#[tauri::command]
fn get_initial_state(state: tauri::State<'_, SharedState>) -> AppStateSnapshot {
    state.lock().snapshot.clone()
}

#[tauri::command]
fn save_text(text: String, state: tauri::State<'_, SharedState>) -> Result<(), String> {
    let snapshot = {
        let mut inner = state.lock();
        inner.snapshot.text = text;
        let mut snap = inner.snapshot.clone();
        snap.pose = PoseDto {
            x: inner.controller.pose.x,
            y: inner.controller.pose.y,
            w: inner.controller.pose.w,
            h: inner.controller.pose.h,
        };
        snap.pinned = inner.controller.pose.pinned;
        inner.snapshot = snap.clone();
        snap
    };
    save_state(&snapshot)
}

#[tauri::command]
fn set_pinned(pinned: bool, state: tauri::State<'_, SharedState>, app: AppHandle) -> bool {
    let mut inner = state.lock();
    inner.controller.set_pinned(pinned);
    inner.snapshot.pinned = pinned;
    persist_from_controller(&inner);
    let _ = app.emit("pinned-changed", pinned);
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
            inner
                .controller
                .end_drag(pos.x as f64, pos.y as f64);
        }
        persist_from_controller(&inner);
    }
}

#[tauri::command]
fn notify_resized(w: f64, h: f64, state: tauri::State<'_, SharedState>) {
    let mut inner = state.lock();
    inner.controller.set_size(w, h);
    persist_from_controller(&inner);
}

#[tauri::command]
fn check_accessibility() -> bool {
    permissions::is_accessibility_trusted(false)
}

#[tauri::command]
fn open_accessibility_settings() {
    permissions::open_accessibility_settings();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let snapshot = persist::load_state();
    let pose = snapshot.to_pose();
    let controller = InteractionController::new(
        pose,
        DesktopLayout::default(),
        InteractionParams::default(),
    );

    let shared: SharedState = Arc::new(Mutex::new(AppInner {
        controller,
        snapshot,
        mouse: MouseTracker::new(),
        permission_prompted: false,
    }));

    let shared_for_setup = shared.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(shared.clone())
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
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let window = app
                .get_webview_window("main")
                .expect("main window missing");

            {
                let mut inner = shared_for_setup.lock();
                let layout = build_layout(&window);
                inner.controller.set_layout(layout);
                let pose = inner.controller.pose;
                apply_pose(&window, &pose);
                let _ = window.set_always_on_top(true);
            }

            // Tray menu
            let pin_i = MenuItem::with_id(app, "pin", "Pin", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Show / Hide", true, None::<&str>)?;
            let reset_i = MenuItem::with_id(app, "reset", "Reset position", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&pin_i, &show_i, &reset_i, &quit_i])?;

            let state_tray = shared_for_setup.clone();
            let handle_tray = handle.clone();
            let _tray = TrayIconBuilder::new()
                .menu(&menu)
                .tooltip("shy-notes")
                .on_menu_event(move |_app, event| match event.id.as_ref() {
                    "pin" => {
                        let pinned = {
                            let mut inner = state_tray.lock();
                            let next = !inner.controller.pose.pinned;
                            inner.controller.set_pinned(next);
                            inner.snapshot.pinned = next;
                            persist_from_controller(&inner);
                            next
                        };
                        let _ = handle_tray.emit("pinned-changed", pinned);
                    }
                    "show" => toggle_visibility(&handle_tray),
                    "reset" => reset_position(&handle_tray, &state_tray),
                    "quit" => {
                        {
                            let inner = state_tray.lock();
                            persist_from_controller(&inner);
                        }
                        handle_tray.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { .. } = event {
                        toggle_visibility(tray.app_handle());
                    }
                })
                .build(app)?;

            // Global shortcut: Cmd/Ctrl+Shift+Space
            let shortcut = if cfg!(target_os = "macos") {
                Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::Space)
            } else {
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space)
            };
            let handle_gs = handle.clone();
            app.global_shortcut().on_shortcut(shortcut, move |_app, _sc, event| {
                if event.state == ShortcutState::Pressed {
                    toggle_visibility(&handle_gs);
                }
            })?;

            // Mouse / interaction loop
            let loop_state = shared_for_setup.clone();
            let loop_handle = handle.clone();
            std::thread::spawn(move || {
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

                    let layout = build_layout(&window);
                    let scale = window.scale_factor().unwrap_or(1.0);
                    let cmds = {
                        let mut inner = loop_state.lock();
                        inner.controller.set_layout(layout);
                        // Keep core pose aligned with the real window (physical pixels).
                        if let (Ok(pos), Ok(size)) = (window.outer_position(), window.outer_size()) {
                            inner.controller.sync_outer_pose(
                                pos.x as f64,
                                pos.y as f64,
                                size.width as f64,
                                size.height as f64,
                            );
                        }
                        // device_query reports Cocoa points on macOS; core uses physical pixels.
                        let mut sample = inner.mouse.sample();
                        sample.x_phys *= scale;
                        sample.y_phys *= scale;
                        inner.controller.on_mouse(sample)
                    };
                    apply_commands(&window, &loop_handle, &cmds);
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building shy-notes")
        .run(|app_handle, event| {
            if let RunEvent::ExitRequested { .. } | RunEvent::Exit = event {
                if let Some(state) = app_handle.try_state::<SharedState>() {
                    let inner = state.lock();
                    persist_from_controller(&inner);
                }
            }
        });
}
