//! Explicit interaction state machine.

use crate::geometry::{
    capture_center, clamp_pose_to_layout, point_in_circle, ray_hits_circle, repulsion_delta,
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
    last_mouse: Option<MouseSample>,
    pre_capture_since_ms: Option<u64>,
    target_x: f64,
    target_y: f64,
    /// Consecutive samples with the pointer outside the widget while Captured.
    outside_capture_ticks: u32,
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
            last_mouse: None,
            pre_capture_since_ms: None,
            target_x: pose.x,
            target_y: pose.y,
            outside_capture_ticks: 0,
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
            // Capture-for-editing still works while pinned; focus once when entering the surface.
            let inside = point_in_circle(sample.x_phys, sample.y_phys, cx, cy, r);
            let was_inside = self
                .last_mouse
                .map(|m| point_in_circle(m.x_phys, m.y_phys, cx, cy, r))
                .unwrap_or(false);
            if inside && !was_inside {
                cmds.push(InteractionCommand::RequestFocus);
            }
            self.last_mouse = Some(sample);
            return cmds;
        }

        if !self.evasion_enabled {
            self.last_mouse = Some(sample);
            return cmds;
        }

        let (vx, vy, speed) = match self.last_mouse {
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
                // ~300ms at 60Hz — pointer must clearly leave before releasing edit mode.
                if self.outside_capture_ticks >= 18 {
                    self.state = InteractionState::Idle;
                    self.outside_capture_ticks = 0;
                    if self.visuals.pre_capture {
                        self.visuals.pre_capture = false;
                        cmds.push(InteractionCommand::SetVisual {
                            pre_capture: false,
                        });
                    }
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
            if self.visuals.pre_capture {
                self.visuals.pre_capture = false;
                cmds.push(InteractionCommand::SetVisual {
                    pre_capture: false,
                });
            }
            cmds.push(InteractionCommand::RequestFocus);
            self.last_mouse = Some(sample);
            return cmds;
        }

        // Outside influence: idle.
        if dist_center > self.params.influence_radius + (self.pose.w.max(self.pose.h) * 0.5) {
            if self.state != InteractionState::Idle {
                self.state = InteractionState::Idle;
            }
            if self.visuals.pre_capture {
                self.visuals.pre_capture = false;
                self.pre_capture_since_ms = None;
                cmds.push(InteractionCommand::SetVisual {
                    pre_capture: false,
                });
            }
            self.last_mouse = Some(sample);
            return cmds;
        }

        self.state = InteractionState::Evaluating;

        // Ambiguous / nearly still → do not move.
        if speed < self.params.still_speed_threshold {
            if self.visuals.pre_capture {
                // Keep pre-capture if already aiming, else clear.
            } else {
                self.state = InteractionState::Idle;
            }
            self.last_mouse = Some(sample);
            return cmds;
        }

        let mag = speed.max(1.0);
        let dx = vx / mag;
        let dy = vy / mag;
        // Look far enough to reach the capture surface when aiming at it.
        let reach = dist_center + capture_r + self.params.look_ahead_px;
        let aims_capture = ray_hits_circle(
            sample.x_phys,
            sample.y_phys,
            dx,
            dy,
            reach,
            cx,
            cy,
            capture_r,
        );

        if aims_capture {
            // Case A: pre-capture then hold still.
            if self.pre_capture_since_ms.is_none() {
                self.pre_capture_since_ms = Some(sample.t_ms);
            }
            self.state = InteractionState::PreCapture;
            if !self.visuals.pre_capture
                && sample.t_ms.saturating_sub(self.pre_capture_since_ms.unwrap_or(sample.t_ms))
                    >= self.params.pre_capture_delay_ms / 4
            {
                self.visuals.pre_capture = true;
                cmds.push(InteractionCommand::SetVisual {
                    pre_capture: true,
                });
            }
            self.last_mouse = Some(sample);
            return cmds;
        }

        // Case B: repel.
        self.pre_capture_since_ms = None;
        if self.visuals.pre_capture {
            self.visuals.pre_capture = false;
            cmds.push(InteractionCommand::SetVisual {
                pre_capture: false,
            });
        }
        self.state = InteractionState::Repelling;

        let (ax, ay) = repulsion_delta(
            &self.pose,
            sample.x_phys,
            sample.y_phys,
            vx,
            vy,
            self.params.repulsion_strength,
            self.params.repulsion_max_step,
        );

        self.target_x += ax;
        self.target_y += ay;

        let mut next = self.pose;
        next.x += (self.target_x - self.pose.x) * self.params.animation_lerp;
        next.y += (self.target_y - self.pose.y) * self.params.animation_lerp;
        next = clamp_pose_to_layout(&next, &self.layout);
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
    fn still_mouse_does_not_flee() {
        let mut c = InteractionController::new(pose_center(), layout(), InteractionParams::default());
        let s0 = MouseSample {
            t_ms: 0,
            x_phys: 700.0,
            y_phys: 540.0,
        };
        let s1 = MouseSample {
            t_ms: 16,
            x_phys: 700.05,
            y_phys: 540.0,
        };
        c.on_mouse(s0);
        let cmds = c.on_mouse(s1);
        assert!(cmds.iter().all(|c| !matches!(c, InteractionCommand::SetPose { .. })));
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
