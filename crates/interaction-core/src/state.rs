//! Explicit interaction state machine.

use crate::geometry::{
    aims_at_capture, capture_center, capture_miss_factor, clamp_pose_to_layout, point_in_circle,
    repulsion_delta, resolve_flee_pose,
};
use crate::params::InteractionParams;
use crate::types::{
    DesktopLayout, InteractionCommand, MouseSample, VisualHints, WidgetPose,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionState {
    Idle,
    Evaluating,
    PreCapture,
    Captured,
    Repelling,
    Dragging,
    Pinned,
}

#[derive(Debug, Clone)]
pub struct InteractionController {
    pub params: InteractionParams,
    pub state: InteractionState,
    pub pose: WidgetPose,
    pub layout: DesktopLayout,
    pub visuals: VisualHints,
    pub evasion_enabled: bool,
    /// When true (e.g. Ctrl held), never flee — capture / pre-capture still work.
    pub suppress_repulsion: bool,
    last_mouse: Option<MouseSample>,
    pre_capture_since_ms: Option<u64>,
    target_x: f64,
    target_y: f64,
    /// Consecutive samples with the pointer outside the widget while Captured.
    outside_capture_ticks: u32,
    /// Consecutive non-aim samples while in PreCapture (stickiness).
    miss_aim_ticks: u32,
    /// After a corner/edge ring swap, suppress further swaps until this time (ms).
    corner_swap_cooldown_until_ms: u64,
}

impl InteractionController {
    pub fn new(pose: WidgetPose, layout: DesktopLayout, params: InteractionParams) -> Self {
        let pose = clamp_pose_to_layout(&pose, &layout);
        Self {
            params,
            state: if pose.pinned {
                InteractionState::Pinned
            } else {
                InteractionState::Idle
            },
            pose,
            layout,
            visuals: VisualHints::default(),
            evasion_enabled: true,
            suppress_repulsion: false,
            last_mouse: None,
            pre_capture_since_ms: None,
            target_x: pose.x,
            target_y: pose.y,
            outside_capture_ticks: 0,
            miss_aim_ticks: 0,
            corner_swap_cooldown_until_ms: 0,
        }
    }

    /// Sync pose from the real window bounds (physical pixels) without clamping jitter.
    pub fn sync_outer_pose(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.pose.x = x;
        self.pose.y = y;
        self.pose.w = w.max(120.0);
        self.pose.h = h.max(80.0);
        if self.state != InteractionState::Repelling {
            self.target_x = x;
            self.target_y = y;
        }
    }

    pub fn set_layout(&mut self, layout: DesktopLayout) {
        self.layout = layout;
        self.pose = clamp_pose_to_layout(&self.pose, &self.layout);
        self.target_x = self.pose.x;
        self.target_y = self.pose.y;
    }

    pub fn set_pinned(&mut self, pinned: bool) {
        self.pose.pinned = pinned;
        if pinned {
            self.state = InteractionState::Pinned;
            self.visuals.pre_capture = false;
            self.pre_capture_since_ms = None;
        } else if self.state == InteractionState::Pinned {
            self.state = InteractionState::Idle;
        }
    }

    pub fn begin_drag(&mut self) {
        self.state = InteractionState::Dragging;
        self.visuals.pre_capture = false;
        self.pre_capture_since_ms = None;
        self.outside_capture_ticks = 0;
        self.miss_aim_ticks = 0;
    }

    pub fn end_drag(&mut self, x: f64, y: f64) {
        self.pose.x = x;
        self.pose.y = y;
        self.pose = clamp_pose_to_layout(&self.pose, &self.layout);
        self.target_x = self.pose.x;
        self.target_y = self.pose.y;
        self.state = if self.pose.pinned {
            InteractionState::Pinned
        } else {
            InteractionState::Idle
        };
    }

    pub fn set_size(&mut self, w: f64, h: f64) {
        self.pose.w = w.max(120.0);
        self.pose.h = h.max(80.0);
        self.pose = clamp_pose_to_layout(&self.pose, &self.layout);
    }

    fn emit_visual(
        &mut self,
        cmds: &mut Vec<InteractionCommand>,
        pre_capture: bool,
        glow: f32,
    ) {
        let glow = glow.clamp(0.0, 1.0);
        if self.visuals.pre_capture == pre_capture && (self.visuals.glow - glow).abs() < 0.02 {
            return;
        }
        self.visuals.pre_capture = pre_capture;
        self.visuals.glow = glow;
        cmds.push(InteractionCommand::SetVisual {
            pre_capture,
            glow,
        });
    }

    fn approach_glow(&self, dist_center: f64, capture_r: f64) -> f32 {
        let outer = self.params.influence_radius + self.pose.w.max(self.pose.h) * 0.5;
        let edge = (dist_center - capture_r).max(0.0);
        let span = (outer - capture_r).max(1.0);
        (1.0 - (edge / span) as f32).clamp(0.0, 1.0)
    }

    /// Drive one tick from a mouse sample. Returns commands for the native adapter.
    pub fn on_mouse(&mut self, sample: MouseSample) -> Vec<InteractionCommand> {
        let mut cmds = Vec::new();

        if self.state == InteractionState::Dragging {
            self.last_mouse = Some(sample);
            return cmds;
        }

        if self.pose.pinned || self.state == InteractionState::Pinned {
            self.state = InteractionState::Pinned;
            let (cx, cy) = capture_center(&self.pose);
            let r = self.params.capture_diameter * 0.5;
            let inside = point_in_circle(sample.x_phys, sample.y_phys, cx, cy, r);
            let was_inside = self
                .last_mouse
                .map(|m| point_in_circle(m.x_phys, m.y_phys, cx, cy, r))
                .unwrap_or(false);
            if inside && !was_inside {
                cmds.push(InteractionCommand::RequestFocus);
            }
            self.emit_visual(&mut cmds, false, 0.0);
            self.last_mouse = Some(sample);
            return cmds;
        }

        if !self.evasion_enabled {
            self.last_mouse = Some(sample);
            return cmds;
        }

        let (vx, vy, _speed) = match self.last_mouse {
            Some(prev) if sample.t_ms > prev.t_ms => {
                let dt = (sample.t_ms - prev.t_ms) as f64 / 1000.0;
                let vx = (sample.x_phys - prev.x_phys) / dt.max(0.001);
                let vy = (sample.y_phys - prev.y_phys) / dt.max(0.001);
                let speed = (vx * vx + vy * vy).sqrt();
                (vx, vy, speed)
            }
            _ => (0.0, 0.0, 0.0),
        };

        let (cx, cy) = capture_center(&self.pose);
        let capture_r = self.params.capture_diameter * 0.5;
        let dist_center = (sample.x_phys - cx).hypot(sample.y_phys - cy);
        let influence_outer =
            self.params.influence_radius + self.pose.w.max(self.pose.h) * 0.5;

        // Leave capture only after sustained exit (avoids DPI/jitter dropping edit mode).
        if self.state == InteractionState::Captured {
            let margin = (self.pose.w.min(self.pose.h) * 0.15).clamp(24.0, 64.0);
            let inside = sample.x_phys >= self.pose.x - margin
                && sample.y_phys >= self.pose.y - margin
                && sample.x_phys <= self.pose.x + self.pose.w + margin
                && sample.y_phys <= self.pose.y + self.pose.h + margin;
            if inside {
                self.outside_capture_ticks = 0;
            } else {
                self.outside_capture_ticks = self.outside_capture_ticks.saturating_add(1);
                if self.outside_capture_ticks >= 18 {
                    self.state = InteractionState::Idle;
                    self.outside_capture_ticks = 0;
                    self.emit_visual(&mut cmds, false, 0.0);
                }
            }
            self.last_mouse = Some(sample);
            return cmds;
        }

        // Auto-capture when pointer reaches capture surface.
        if point_in_circle(sample.x_phys, sample.y_phys, cx, cy, capture_r) {
            self.state = InteractionState::Captured;
            self.outside_capture_ticks = 0;
            self.pre_capture_since_ms = None;
            self.emit_visual(&mut cmds, false, 0.0);
            cmds.push(InteractionCommand::RequestFocus);
            self.last_mouse = Some(sample);
            return cmds;
        }

        // Outside influence: idle.
        if dist_center > influence_outer {
            if self.state != InteractionState::Idle {
                self.state = InteractionState::Idle;
            }
            self.pre_capture_since_ms = None;
            self.emit_visual(&mut cmds, false, 0.0);
            self.last_mouse = Some(sample);
            return cmds;
        }

        // Outer fringe: distance falloff would be ~0 — treat as idle so we do not
        // sit in Repelling with an invisible nudge.
        let near_dist = (self.params.capture_diameter * 0.5).max(48.0);
        let span = (influence_outer - near_dist).max(1.0);
        let proximity = ((influence_outer - dist_center) / span).clamp(0.0, 1.0);
        if proximity < 0.12 {
            if self.state != InteractionState::Idle {
                self.state = InteractionState::Idle;
            }
            self.pre_capture_since_ms = None;
            self.emit_visual(&mut cmds, false, 0.0);
            self.last_mouse = Some(sample);
            return cmds;
        }

        let glow = self.approach_glow(dist_center, capture_r);
        let was_precapture = self.state == InteractionState::PreCapture
            || self.pre_capture_since_ms.is_some()
            || self.visuals.pre_capture;
        self.state = InteractionState::Evaluating;

        // Need at least one prior sample before fleeing — otherwise the first
        // frame of an approach can't detect aim yet and would wrongly push away.
        if self.last_mouse.is_none() {
            self.emit_visual(&mut cmds, false, glow * 0.5);
            self.last_mouse = Some(sample);
            return cmds;
        }

        // Aim toward capture: generous cone + enlarged ray. Sticky once engaged,
        // but a clear miss cancels stickiness immediately.
        let aim_min_speed = (self.params.still_speed_threshold * 0.35).max(4.0);
        let raw_aims = aims_at_capture(
            sample.x_phys,
            sample.y_phys,
            vx,
            vy,
            cx,
            cy,
            capture_r,
            self.params.look_ahead_px,
            aim_min_speed,
        );
        let miss = capture_miss_factor(
            sample.x_phys,
            sample.y_phys,
            vx,
            vy,
            cx,
            cy,
            capture_r,
            aim_min_speed,
        );
        let clear_miss = miss >= 0.55;

        let aims_capture = if raw_aims {
            self.miss_aim_ticks = 0;
            true
        } else if was_precapture && !clear_miss {
            self.miss_aim_ticks = self.miss_aim_ticks.saturating_add(1);
            self.miss_aim_ticks < 8
        } else {
            self.miss_aim_ticks = 0;
            false
        };

        if aims_capture {
            if self.pre_capture_since_ms.is_none() {
                self.pre_capture_since_ms = Some(sample.t_ms);
            }
            self.state = InteractionState::PreCapture;
            let ready = sample
                .t_ms
                .saturating_sub(self.pre_capture_since_ms.unwrap_or(sample.t_ms))
                >= self.params.pre_capture_delay_ms / 4;
            self.emit_visual(&mut cmds, ready, glow.max(0.45));
            self.last_mouse = Some(sample);
            return cmds;
        }

        // Case B: not aiming → flee (unless temporarily suppressed, e.g. Ctrl held).
        self.pre_capture_since_ms = None;
        self.miss_aim_ticks = 0;
        self.state = InteractionState::Repelling;
        self.emit_visual(&mut cmds, false, glow * 0.35);

        if self.suppress_repulsion {
            self.last_mouse = Some(sample);
            return cmds;
        }

        // Mild miss boost — peak force still comes from prefs max_step × falloff.
        let boost = 1.0 + miss * 0.35;
        let capture_r = self.params.capture_diameter * 0.5;
        let near_dist = capture_r.max(48.0);
        // `repulsion_max_step` is the peak (slider); distance falloff scales it down.
        let peak = self.params.repulsion_max_step * boost;
        let (ax, ay) = repulsion_delta(
            &self.pose,
            sample.x_phys,
            sample.y_phys,
            vx,
            vy,
            peak,
            influence_outer,
            near_dist,
        );

        let before = self.pose;
        self.target_x += ax;
        self.target_y += ay;

        let mut next = self.pose;
        next.x += (self.target_x - self.pose.x) * self.params.animation_lerp;
        next.y += (self.target_y - self.pose.y) * self.params.animation_lerp;
        next = clamp_pose_to_layout(&next, &self.layout);

        // If flee would pin us in a corner / against a blocked edge, swap along
        // the repulsion circumference around the pointer — with cooldown so a
        // stationary cursor on the switch boundary cannot flip-flop.
        let ring = self.params.influence_radius.max(280.0);
        let proposed = next;
        let resolved = resolve_flee_pose(
            &before,
            &proposed,
            sample.x_phys,
            sample.y_phys,
            ax,
            ay,
            &self.layout,
            ring,
        );
        let swapped = (resolved.x - proposed.x).abs() > 1.0 || (resolved.y - proposed.y).abs() > 1.0;
        if swapped {
            if sample.t_ms < self.corner_swap_cooldown_until_ms {
                // Still in the hysteresis window — keep the clamped flee step.
                next = proposed;
            } else {
                next = resolved;
                // ~1s at 60Hz; long enough that leaving the cursor still won't oscillate.
                self.corner_swap_cooldown_until_ms = sample.t_ms.saturating_add(1000);
            }
        } else {
            next = resolved;
        }

        self.target_x = next.x;
        self.target_y = next.y;

        if (next.x - self.pose.x).abs() > 0.05 || (next.y - self.pose.y).abs() > 0.05 {
            self.pose.x = next.x;
            self.pose.y = next.y;
            cmds.push(InteractionCommand::SetPose {
                x: self.pose.x,
                y: self.pose.y,
            });
        }

        self.last_mouse = Some(sample);
        cmds
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MonitorInfo;

    fn layout() -> DesktopLayout {
        DesktopLayout {
            monitors: vec![MonitorInfo {
                id: "m".into(),
                x: 0.0,
                y: 0.0,
                w: 1920.0,
                h: 1080.0,
                scale: 1.0,
            }],
        }
    }

    fn pose_center() -> WidgetPose {
        WidgetPose {
            x: 860.0,
            y: 440.0,
            w: 200.0,
            h: 200.0,
            pinned: false,
        }
    }

    #[test]
    fn still_mouse_in_influence_flees_away() {
        let mut c = InteractionController::new(pose_center(), layout(), InteractionParams::default());
        let start_x = c.pose.x;
        // Nearly still, left of center → must push right (away), never left (toward).
        c.on_mouse(MouseSample {
            t_ms: 0,
            x_phys: 700.0,
            y_phys: 540.0,
        });
        let cmds = c.on_mouse(MouseSample {
            t_ms: 16,
            x_phys: 700.05,
            y_phys: 540.0,
        });
        assert!(
            cmds.iter()
                .any(|c| matches!(c, InteractionCommand::SetPose { .. })),
            "inside influence without aiming should flee"
        );
        assert!(c.pose.x > start_x, "must flee away from pointer on the left");
    }

    #[test]
    fn approaching_from_right_flees_left() {
        let mut c = InteractionController::new(pose_center(), layout(), InteractionParams::default());
        let start_x = c.pose.x;
        // Glancing above while approaching from the right — not aimed at capture.
        c.on_mouse(MouseSample {
            t_ms: 0,
            x_phys: 1200.0,
            y_phys: 300.0,
        });
        c.on_mouse(MouseSample {
            t_ms: 16,
            x_phys: 1120.0,
            y_phys: 300.0,
        });
        assert!(c.pose.x < start_x, "must flee left when pointer is on the right");
    }

    #[test]
    fn trajectory_to_capture_does_not_repel() {
        let mut c = InteractionController::new(pose_center(), layout(), InteractionParams::default());
        // Approach from left toward center (960, 540).
        c.on_mouse(MouseSample {
            t_ms: 0,
            x_phys: 600.0,
            y_phys: 540.0,
        });
        let cmds = c.on_mouse(MouseSample {
            t_ms: 16,
            x_phys: 650.0,
            y_phys: 540.0,
        });
        assert!(
            !cmds
                .iter()
                .any(|c| matches!(c, InteractionCommand::SetPose { .. })),
            "should not flee when aiming at capture"
        );
        assert_eq!(c.state, InteractionState::PreCapture);
    }

    #[test]
    fn glancing_trajectory_repels() {
        let mut c = InteractionController::new(pose_center(), layout(), InteractionParams::default());
        // Pass above the widget, not toward center.
        c.on_mouse(MouseSample {
            t_ms: 0,
            x_phys: 700.0,
            y_phys: 300.0,
        });
        let cmds = c.on_mouse(MouseSample {
            t_ms: 16,
            x_phys: 780.0,
            y_phys: 300.0,
        });
        assert!(
            cmds
                .iter()
                .any(|c| matches!(c, InteractionCommand::SetPose { .. })),
            "should flee glancing approach"
        );
        assert_eq!(c.state, InteractionState::Repelling);
    }

    #[test]
    fn enter_capture_surface_captures() {
        let mut c = InteractionController::new(pose_center(), layout(), InteractionParams::default());
        let (cx, cy) = capture_center(&c.pose);
        let cmds = c.on_mouse(MouseSample {
            t_ms: 10,
            x_phys: cx,
            y_phys: cy,
        });
        assert_eq!(c.state, InteractionState::Captured);
        assert!(cmds
            .iter()
            .any(|c| matches!(c, InteractionCommand::RequestFocus)));
    }

    #[test]
    fn pinned_never_moves() {
        let mut pose = pose_center();
        pose.pinned = true;
        let mut c = InteractionController::new(pose, layout(), InteractionParams::default());
        c.on_mouse(MouseSample {
            t_ms: 0,
            x_phys: 700.0,
            y_phys: 300.0,
        });
        let cmds = c.on_mouse(MouseSample {
            t_ms: 16,
            x_phys: 780.0,
            y_phys: 300.0,
        });
        assert!(cmds
            .iter()
            .all(|c| !matches!(c, InteractionCommand::SetPose { .. })));
        assert_eq!(c.state, InteractionState::Pinned);
    }

    #[test]
    fn corner_swap_cooldown_prevents_flip_flop() {
        let mut pose = WidgetPose {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
            pinned: false,
        };
        let mut c = InteractionController::new(pose, layout(), InteractionParams::default());
        // Pressure into the top-left corner until a ring swap fires.
        let mut t = 0u64;
        let mut swapped_once = false;
        for _ in 0..90 {
            t += 16;
            c.on_mouse(MouseSample {
                t_ms: t,
                x_phys: 60.0,
                y_phys: 60.0,
            });
            if c.pose.x > 200.0 || c.pose.y > 200.0 {
                swapped_once = true;
                break;
            }
        }
        assert!(swapped_once, "expected an initial corner swap");
        pose = c.pose;
        // Hold the pointer still through the cooldown window — must not leap away again.
        let mut max_jump = 0.0f64;
        for _ in 0..50 {
            t += 16;
            let before = c.pose;
            c.on_mouse(MouseSample {
                t_ms: t,
                x_phys: 60.0,
                y_phys: 60.0,
            });
            let jump = (c.pose.x - before.x).hypot(c.pose.y - before.y);
            max_jump = max_jump.max(jump);
        }
        assert!(
            max_jump < 120.0,
            "cooldown should block ring flip-flop (max jump {max_jump}, from {:?})",
            pose
        );
    }

    #[test]
    fn drag_priority_suppresses_repulsion() {
        let mut c = InteractionController::new(pose_center(), layout(), InteractionParams::default());
        c.begin_drag();
        c.on_mouse(MouseSample {
            t_ms: 0,
            x_phys: 700.0,
            y_phys: 300.0,
        });
        let cmds = c.on_mouse(MouseSample {
            t_ms: 16,
            x_phys: 780.0,
            y_phys: 300.0,
        });
        assert!(cmds.is_empty());
        assert_eq!(c.state, InteractionState::Dragging);
    }
}
