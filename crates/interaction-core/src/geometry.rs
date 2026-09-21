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
    let fx = cx - ox;
    let fy = cy - oy;
    let proj = (fx * dx + fy * dy).clamp(0.0, look_ahead);
    let closest_x = ox + dx * proj;
    let closest_y = oy + dy * proj;
    point_in_circle(closest_x, closest_y, cx, cy, radius)
}

/// True when the pointer motion is directed toward the capture surface.
///
/// Uses a generous cone toward the center plus an enlarged ray-circle test so
/// small tracking noise does not flip the widget into repulsion mid-approach.
#[allow(clippy::too_many_arguments)]
pub fn aims_at_capture(
    mx: f64,
    my: f64,
    vx: f64,
    vy: f64,
    cx: f64,
    cy: f64,
    capture_r: f64,
    look_ahead: f64,
    min_speed: f64,
) -> bool {
    let speed = (vx * vx + vy * vy).sqrt();
    if speed < min_speed {
        return false;
    }

    let to_x = cx - mx;
    let to_y = cy - my;
    let to_len = (to_x * to_x + to_y * to_y).sqrt().max(1.0);
    let ux = vx / speed;
    let uy = vy / speed;
    // cos(42°) ≈ 0.74 — wide enough for natural hand aiming.
    let cos_to_center = (ux * to_x + uy * to_y) / to_len;
    if cos_to_center >= 0.74 {
        return true;
    }

    let aim_r = capture_r * 1.75;
    let reach = to_len + aim_r + look_ahead;
    ray_hits_circle(mx, my, ux, uy, reach, cx, cy, aim_r)
}

/// How clearly the motion misses the capture surface: `0` = aiming, `1` = clear miss.
#[allow(clippy::too_many_arguments)]
pub fn capture_miss_factor(
    mx: f64,
    my: f64,
    vx: f64,
    vy: f64,
    cx: f64,
    cy: f64,
    capture_r: f64,
    min_speed: f64,
) -> f64 {
    let speed = (vx * vx + vy * vy).sqrt();
    let to_x = cx - mx;
    let to_y = cy - my;
    let to_len = (to_x * to_x + to_y * to_y).sqrt().max(1.0);

    if speed < min_speed {
        // Hovering off-center counts as a miss; nearer the capture = less.
        return ((to_len - capture_r) / (capture_r * 4.0)).clamp(0.0, 1.0);
    }

    let ux = vx / speed;
    let uy = vy / speed;
    let cos_to_center = (ux * to_x + uy * to_y) / to_len;
    // Map cos from aiming (~0.74+) → 0, through orthogonal/back → 1.
    let cone_miss = ((0.74 - cos_to_center) / 1.2).clamp(0.0, 1.0);

    // Perpendicular distance from the motion ray to the capture center.
    let cross = (ux * to_y - uy * to_x).abs();
    let lateral_miss = ((cross - capture_r) / (capture_r * 3.0)).clamp(0.0, 1.0);

    cone_miss.max(lateral_miss)
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
    // Prefer a decisive jump: use most of max_step immediately.
    let proximity = ((1.0 / dist) * 240.0 * strength).min(max_step);
    let push = proximity.max(max_step * 0.55);
    (dx * push.min(max_step), dy * push.min(max_step))
}

/// Flee strictly away from the pointer. Velocity is ignored on purpose: blending
/// with "-velocity" inverted the flee direction when the cursor approached.
pub fn repulsion_delta(
    pose: &WidgetPose,
    mx: f64,
    my: f64,
    _vx: f64,
    _vy: f64,
    strength: f64,
    max_step: f64,
) -> (f64, f64) {
    soft_away_from_point(pose, mx, my, strength, max_step)
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

/// True when the pose sits in a monitor corner (near two perpendicular edges).
pub fn is_pose_in_corner(pose: &WidgetPose, layout: &DesktopLayout, margin: f64) -> bool {
    for m in &layout.monitors {
        let fits = pose.x >= m.x - 1.0
            && pose.y >= m.y - 1.0
            && pose.x + pose.w <= m.x + m.w + 1.0
            && pose.y + pose.h <= m.y + m.h + 1.0;
        if !fits {
            continue;
        }
        let near_l = pose.x <= m.x + margin;
        let near_r = pose.x + pose.w >= m.x + m.w - margin;
        let near_t = pose.y <= m.y + margin;
        let near_b = pose.y + pose.h >= m.y + m.h - margin;
        if (near_l || near_r) && (near_t || near_b) {
            return true;
        }
    }
    false
}

/// Pick an alternate on-screen pose on the repulsion ring around the pointer.
///
/// Used when a normal flee would trap the widget in a corner (or against an edge
/// with nowhere left to go). Samples the circumference and chooses a free spot.
pub fn swap_along_repulsion_ring(
    pose: &WidgetPose,
    mx: f64,
    my: f64,
    layout: &DesktopLayout,
    ring_radius: f64,
) -> WidgetPose {
    const SAMPLES: usize = 36;
    let (ocx, ocy) = pose.center();
    let base = (ocy - my).atan2(ocx - mx);
    let ring = ring_radius.max(pose.w.max(pose.h) + 40.0);

    let mut best = *pose;
    let mut best_score = f64::NEG_INFINITY;

    for i in 0..SAMPLES {
        let angle = base + (i as f64) * (std::f64::consts::TAU / SAMPLES as f64);
        let cx = mx + angle.cos() * ring;
        let cy = my + angle.sin() * ring;
        let mut cand = *pose;
        cand.x = cx - pose.w * 0.5;
        cand.y = cy - pose.h * 0.5;
        cand = clamp_pose_to_layout(&cand, layout);

        let (ccx, ccy) = cand.center();
        let dist = (ccx - mx).hypot(ccy - my);
        let moved = (ccx - ocx).hypot(ccy - ocy);
        let corner = is_pose_in_corner(&cand, layout, 56.0);
        // Skip candidates that stay in a corner unless nothing else exists.
        let corner_penalty = if corner { 50_000.0 } else { 0.0 };
        // Prefer clear distance from the pointer and a real relocation.
        let score = dist * 3.0 + moved * 1.5 - corner_penalty;
        if score > best_score {
            best_score = score;
            best = cand;
        }
    }

    best
}

/// After a flee step, relocate along the repulsion ring if trapped in a corner
/// or if the requested push was almost entirely blocked by screen edges.
pub fn resolve_flee_pose(
    before: &WidgetPose,
    proposed: &WidgetPose,
    mx: f64,
    my: f64,
    desired_dx: f64,
    desired_dy: f64,
    layout: &DesktopLayout,
    ring_radius: f64,
) -> WidgetPose {
    let (bx, by) = before.center();
    let (px, py) = proposed.center();
    let moved = (px - bx).hypot(py - by);
    let desired = desired_dx.hypot(desired_dy);
    let blocked = desired > 24.0 && moved < desired * 0.2;
    let cornered = is_pose_in_corner(proposed, layout, 56.0);

    if !cornered && !blocked {
        return *proposed;
    }

    let swapped = swap_along_repulsion_ring(proposed, mx, my, layout, ring_radius);
    // Only accept the swap if it actually improves the situation.
    let (sx, sy) = swapped.center();
    let swap_dist = (sx - mx).hypot(sy - my);
    let prop_dist = (px - mx).hypot(py - my);
    let swap_corner = is_pose_in_corner(&swapped, layout, 56.0);
    if (!swap_corner && (cornered || swap_dist > prop_dist + 8.0))
        || (swap_dist > prop_dist + 40.0)
    {
        return swapped;
    }
    *proposed
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

    #[test]
    fn aims_cone_toward_center() {
        assert!(aims_at_capture(
            600.0, 540.0, 200.0, 0.0, 960.0, 540.0, 50.0, 150.0, 4.0
        ));
        assert!(!aims_at_capture(
            600.0, 200.0, 200.0, 0.0, 960.0, 540.0, 50.0, 150.0, 4.0
        ));
    }

    #[test]
    fn corner_trap_swaps_along_ring() {
        let layout = DesktopLayout {
            monitors: vec![MonitorInfo {
                id: "m".into(),
                x: 0.0,
                y: 0.0,
                w: 1920.0,
                h: 1080.0,
                scale: 1.0,
            }],
        };
        let cornered = WidgetPose {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
            pinned: false,
        };
        assert!(is_pose_in_corner(&cornered, &layout, 56.0));

        // Pointer near the cornered widget — flee would stay trapped without a swap.
        let mx = 80.0;
        let my = 80.0;
        let proposed = cornered;
        let resolved = resolve_flee_pose(
            &cornered,
            &proposed,
            mx,
            my,
            -40.0,
            -40.0,
            &layout,
            400.0,
        );
        assert!(
            !is_pose_in_corner(&resolved, &layout, 56.0),
            "must leave the corner via ring swap"
        );
        let (cx, cy) = resolved.center();
        assert!(
            (cx - mx).hypot(cy - my) > 120.0,
            "swap should keep clear distance from the pointer"
        );
    }
}
