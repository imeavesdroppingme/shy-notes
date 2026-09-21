//! End-to-end session replays against the pure interaction core (no GUI).
//!
//! These simulate realistic cursor paths and assert product-level outcomes:
//! capture without click, flee on glancing approach, pin freeze, stay on-screen.

use interaction_core::{
    capture_center, DesktopLayout, InteractionCommand, InteractionController, InteractionParams,
    InteractionState, MonitorInfo, MouseSample, WidgetPose,
};

fn single_monitor() -> DesktopLayout {
    DesktopLayout {
        monitors: vec![MonitorInfo {
            id: "main".into(),
            x: 0.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
            scale: 1.0,
        }],
    }
}

fn dual_monitor() -> DesktopLayout {
    DesktopLayout {
        monitors: vec![
            MonitorInfo {
                id: "left".into(),
                x: 0.0,
                y: 0.0,
                w: 1920.0,
                h: 1080.0,
                scale: 1.0,
            },
            MonitorInfo {
                id: "right".into(),
                x: 1920.0,
                y: 0.0,
                w: 1920.0,
                h: 1080.0,
                scale: 1.0,
            },
        ],
    }
}

fn widget_at(x: f64, y: f64) -> WidgetPose {
    WidgetPose {
        x,
        y,
        w: 200.0,
        h: 200.0,
        pinned: false,
    }
}

fn sample(t_ms: u64, x: f64, y: f64) -> MouseSample {
    MouseSample {
        t_ms,
        x_phys: x,
        y_phys: y,
    }
}

fn drive(c: &mut InteractionController, path: &[(u64, f64, f64)]) -> Vec<InteractionCommand> {
    let mut all = Vec::new();
    for &(t, x, y) in path {
        all.extend(c.on_mouse(sample(t, x, y)));
    }
    all
}

fn assert_pose_on_layout(c: &InteractionController) {
    assert!(
        c.layout.contains_pose(&c.pose),
        "pose ({}, {}) size {}x{} left the desktop",
        c.pose.x,
        c.pose.y,
        c.pose.w,
        c.pose.h
    );
}

#[test]
fn e2e_aim_at_center_captures_without_click() {
    let mut c = InteractionController::new(widget_at(860.0, 440.0), single_monitor(), InteractionParams::default());
    let (cx, cy) = capture_center(&c.pose);

    // Approach along a horizontal ray aimed at the capture center.
    let cmds = drive(
        &mut c,
        &[
            (0, cx - 320.0, cy),
            (16, cx - 240.0, cy),
            (32, cx - 160.0, cy),
            (48, cx - 80.0, cy),
            (64, cx, cy),
        ],
    );

    assert_eq!(c.state, InteractionState::Captured);
    assert!(
        cmds.iter()
            .any(|cmd| matches!(cmd, InteractionCommand::RequestFocus)),
        "capture must request editor focus"
    );
    assert!(
        !cmds
            .iter()
            .any(|cmd| matches!(cmd, InteractionCommand::SetPose { .. })),
        "aimed approach must not flee"
    );
}

#[test]
fn e2e_glancing_pass_makes_widget_flee_then_stay_visible() {
    let mut c = InteractionController::new(widget_at(860.0, 440.0), single_monitor(), InteractionParams::default());
    let start = (c.pose.x, c.pose.y);

    let cmds = drive(
        &mut c,
        &[
            (0, 700.0, 280.0),
            (16, 780.0, 280.0),
            (32, 860.0, 280.0),
            (48, 940.0, 280.0),
            (64, 1020.0, 280.0),
        ],
    );

    assert!(
        cmds.iter()
            .any(|cmd| matches!(cmd, InteractionCommand::SetPose { .. })),
        "glancing trajectory should trigger repulsion"
    );
    assert!(
        (c.pose.x - start.0).hypot(c.pose.y - start.1) > 1.0,
        "widget should have moved"
    );
    assert_pose_on_layout(&c);
}

#[test]
fn e2e_near_still_mouse_does_not_teleport() {
    let mut c = InteractionController::new(widget_at(860.0, 440.0), single_monitor(), InteractionParams::default());
    let start = (c.pose.x, c.pose.y);

    drive(
        &mut c,
        &[
            (0, 700.0, 540.0),
            (16, 700.02, 540.0),
            (32, 700.03, 540.01),
            (48, 700.04, 540.0),
        ],
    );

    assert!(
        (c.pose.x - start.0).abs() < 0.5 && (c.pose.y - start.1).abs() < 0.5,
        "ambiguous/still motion must prefer staying put"
    );
}

#[test]
fn e2e_leave_widget_releases_capture() {
    let mut c = InteractionController::new(widget_at(860.0, 440.0), single_monitor(), InteractionParams::default());
    let (cx, cy) = capture_center(&c.pose);

    drive(&mut c, &[(0, cx, cy)]);
    assert_eq!(c.state, InteractionState::Captured);

    // Brief excursion must not drop capture (hysteresis).
    drive(&mut c, &[(16, 10.0, 10.0), (32, 10.0, 10.0)]);
    assert_eq!(c.state, InteractionState::Captured);

    // Sustained leave (~18+ ticks) releases edit mode.
    let mut path = Vec::new();
    for i in 0..24 {
        path.push((50 + i * 16, 10.0, 10.0));
    }
    drive(&mut c, &path);
    assert_eq!(c.state, InteractionState::Idle);
}

#[test]
fn e2e_capture_survives_transient_coordinate_glitch() {
    let mut c = InteractionController::new(widget_at(860.0, 440.0), single_monitor(), InteractionParams::default());
    let (cx, cy) = capture_center(&c.pose);
    drive(&mut c, &[(0, cx, cy)]);
    assert_eq!(c.state, InteractionState::Captured);

    // One bad sample far away, then back inside the widget — keep editing.
    drive(
        &mut c,
        &[
            (16, 0.0, 0.0),
            (32, cx, cy),
            (48, cx + 10.0, cy + 10.0),
        ],
    );
    assert_eq!(c.state, InteractionState::Captured);
}

#[test]
fn e2e_pin_freezes_auto_move_but_drag_can_reposition() {
    let mut c = InteractionController::new(widget_at(860.0, 440.0), single_monitor(), InteractionParams::default());
    c.set_pinned(true);
    let pinned_pose = (c.pose.x, c.pose.y);

    drive(
        &mut c,
        &[
            (0, 700.0, 280.0),
            (16, 820.0, 280.0),
            (32, 940.0, 280.0),
        ],
    );
    assert_eq!((c.pose.x, c.pose.y), pinned_pose);
    assert_eq!(c.state, InteractionState::Pinned);

    c.begin_drag();
    c.end_drag(120.0, 140.0);
    assert_eq!(c.pose.x, 120.0);
    assert_eq!(c.pose.y, 140.0);
    assert_eq!(c.state, InteractionState::Pinned);
    assert_pose_on_layout(&c);
}

#[test]
fn e2e_corner_pressure_never_pushes_off_screen() {
    let mut c = InteractionController::new(
        widget_at(10.0, 10.0),
        single_monitor(),
        InteractionParams {
            repulsion_max_step: 40.0,
            ..InteractionParams::default()
        },
    );

    // Push repeatedly toward the top-left corner.
    let mut t = 0_u64;
    for i in 0..40 {
        t += 16;
        let x = 80.0 + i as f64 * 3.0;
        drive(&mut c, &[(t, x, 40.0)]);
        assert_pose_on_layout(&c);
    }
}

#[test]
fn e2e_monitor_unplug_recovers_into_remaining_display() {
    let mut c = InteractionController::new(widget_at(2100.0, 200.0), dual_monitor(), InteractionParams::default());
    assert!(c.layout.contains_pose(&c.pose));

    // Unplug the right monitor — layout shrinks to the left display only.
    c.set_layout(single_monitor());
    assert_pose_on_layout(&c);
    assert!(c.pose.x + c.pose.w <= 1920.0);
    assert!(c.pose.y + c.pose.h <= 1080.0);
}

#[test]
fn e2e_write_interrupt_write_session() {
    // Spec success path: leave widget → glance past → return → capture → write.
    let mut c = InteractionController::new(widget_at(860.0, 440.0), single_monitor(), InteractionParams::default());
    let (cx, cy) = capture_center(&c.pose);

    // 1) Capture and "write".
    drive(&mut c, &[(0, cx, cy)]);
    assert_eq!(c.state, InteractionState::Captured);

    // 2) Leave to work elsewhere (sustained exit for hysteresis).
    let mut leave = Vec::new();
    for i in 0..24 {
        leave.push((20 + i * 16, 100.0, 100.0));
    }
    drive(&mut c, &leave);
    assert_eq!(c.state, InteractionState::Idle);

    // 3) Glance past the widget (should flee).
    let before = (c.pose.x, c.pose.y);
    drive(
        &mut c,
        &[
            (40, 700.0, 260.0),
            (56, 820.0, 260.0),
            (72, 940.0, 260.0),
        ],
    );
    assert!((c.pose.x - before.0).hypot(c.pose.y - before.1) > 0.5);

    // 4) Return deliberately to the (possibly moved) capture center.
    let (cx2, cy2) = capture_center(&c.pose);
    let cmds = drive(
        &mut c,
        &[
            (100, cx2 - 200.0, cy2),
            (116, cx2 - 100.0, cy2),
            (132, cx2, cy2),
        ],
    );
    assert_eq!(c.state, InteractionState::Captured);
    assert!(cmds
        .iter()
        .any(|cmd| matches!(cmd, InteractionCommand::RequestFocus)));
    assert_pose_on_layout(&c);
}
