//! Geometry helpers for capture and repulsion.

use crate::types::{DesktopLayout, WidgetPose};

pub fn capture_center(pose: &WidgetPose) -> (f64, f64) {
    pose.center()
}

pub fn point_in_circle(px: f64, py: f64, cx: f64, cy: f64, radius: f64) -> bool {
    let dx = px - cx;
    let dy = py - cy;
    dx * dx + dy * dy <= radius * radius
}

/// Returns true if the ray from (ox,oy) along unit direction (dx,dy) for length `look_ahead`
/// intersects the circle at (cx,cy) with `radius`.
#[allow(clippy::too_many_arguments)]
pub fn ray_hits_circle(
    ox: f64,
    oy: f64,
    dx: f64,
    dy: f64,
    look_ahead: f64,
    cx: f64,
    cy: f64,
    radius: f64,
) -> bool {
    // Closest point on segment [origin, origin+dir*look_ahead] to circle center.
    let fx = cx - ox;
    let fy = cy - oy;
    let proj = (fx * dx + fy * dy).clamp(0.0, look_ahead);
    let closest_x = ox + dx * proj;
    let closest_y = oy + dy * proj;
    point_in_circle(closest_x, closest_y, cx, cy, radius)
}

pub fn soft_away_from_point(
    pose: &WidgetPose,
    mx: f64,
    my: f64,
    strength: f64,
    max_step: f64,
) -> (f64, f64) {
    let (cx, cy) = pose.center();
    let mut dx = cx - mx;
    let mut dy = cy - my;
    let dist = (dx * dx + dy * dy).sqrt().max(1.0);
    dx /= dist;
    dy /= dist;
    // Stronger when closer.
    let push = ((1.0 / dist) * 120.0 * strength).min(max_step);
    (dx * push, dy * push)
}

pub fn repulsion_delta(
    pose: &WidgetPose,
    mx: f64,
    my: f64,
    vx: f64,
    vy: f64,
    strength: f64,
    max_step: f64,
) -> (f64, f64) {
    let (mut ax, mut ay) = soft_away_from_point(pose, mx, my, strength, max_step);
    // Prefer fleeing opposite to mouse travel.
    let speed = (vx * vx + vy * vy).sqrt();
    if speed > 1.0 {
        let ox = -vx / speed;
        let oy = -vy / speed;
        ax = ax * 0.55 + ox * max_step * 0.45 * strength;
        ay = ay * 0.55 + oy * max_step * 0.45 * strength;
        let mag = (ax * ax + ay * ay).sqrt();
        if mag > max_step {
            ax = ax / mag * max_step;
            ay = ay / mag * max_step;
        }
    }
    (ax, ay)
}

#[allow(clippy::too_many_arguments)]
pub fn rect_contains_rect(
    outer_x: f64,
    outer_y: f64,
    outer_w: f64,
    outer_h: f64,
    inner_x: f64,
    inner_y: f64,
    inner_w: f64,
    inner_h: f64,
) -> bool {
    inner_x >= outer_x
        && inner_y >= outer_y
        && inner_x + inner_w <= outer_x + outer_w
        && inner_y + inner_h <= outer_y + outer_h
}

/// Clamp pose so it lies entirely inside at least one monitor (prefer current, else nearest).
pub fn clamp_pose_to_layout(pose: &WidgetPose, layout: &DesktopLayout) -> WidgetPose {
    if layout.contains_pose(pose) {
        return *pose;
    }
    let mut best = *pose;
    let mut best_dist = f64::INFINITY;
    let (cx, cy) = pose.center();
    for m in &layout.monitors {
        let x = pose.x.clamp(m.x, (m.x + m.w - pose.w).max(m.x));
        let y = pose.y.clamp(m.y, (m.y + m.h - pose.h).max(m.y));
        let nx = x + pose.w * 0.5;
        let ny = y + pose.h * 0.5;
        let d = (nx - cx).hypot(ny - cy);
        if d < best_dist {
            best_dist = d;
            best.x = x;
            best.y = y;
        }
    }
    if layout.monitors.is_empty() {
        let (_bx, _by, bw, bh) = layout.bounds();
        best.x = best.x.clamp(0.0, (bw - pose.w).max(0.0));
        best.y = best.y.clamp(0.0, (bh - pose.h).max(0.0));
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MonitorInfo;

    #[test]
    fn ray_toward_center_hits() {
        assert!(ray_hits_circle(0.0, 0.0, 1.0, 0.0, 200.0, 100.0, 0.0, 50.0));
    }

    #[test]
    fn ray_missing_center_misses() {
        assert!(!ray_hits_circle(0.0, 0.0, 1.0, 0.0, 200.0, 100.0, 200.0, 40.0));
    }

    #[test]
    fn clamp_keeps_on_monitor() {
        let layout = DesktopLayout {
            monitors: vec![MonitorInfo {
                id: "m".into(),
                x: 0.0,
                y: 0.0,
                w: 1000.0,
                h: 800.0,
                scale: 1.0,
            }],
        };
        let pose = WidgetPose {
            x: 950.0,
            y: 750.0,
            w: 200.0,
            h: 150.0,
            pinned: false,
        };
        let c = clamp_pose_to_layout(&pose, &layout);
        assert!(layout.contains_pose(&c));
    }
}
