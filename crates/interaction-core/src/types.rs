//! Shared value types for the interaction core.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MouseSample {
    pub t_ms: u64,
    pub x_phys: f64,
    pub y_phys: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MonitorInfo {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub scale: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DesktopLayout {
    pub monitors: Vec<MonitorInfo>,
}

impl DesktopLayout {
    pub fn primary_or_default(&self) -> MonitorInfo {
        self.monitors.first().cloned().unwrap_or(MonitorInfo {
            id: "default".into(),
            x: 0.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
            scale: 1.0,
        })
    }

    /// Axis-aligned bounding box of all monitors (virtual desktop).
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        if self.monitors.is_empty() {
            return (0.0, 0.0, 1920.0, 1080.0);
        }
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for m in &self.monitors {
            min_x = min_x.min(m.x);
            min_y = min_y.min(m.y);
            max_x = max_x.max(m.x + m.w);
            max_y = max_y.max(m.y + m.h);
        }
        (min_x, min_y, max_x - min_x, max_y - min_y)
    }

    pub fn contains_pose(&self, pose: &WidgetPose) -> bool {
        self.monitors.iter().any(|m| {
            pose.x >= m.x
                && pose.y >= m.y
                && pose.x + pose.w <= m.x + m.w
                && pose.y + pose.h <= m.y + m.h
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WidgetPose {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub pinned: bool,
}

impl WidgetPose {
    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w * 0.5, self.y + self.h * 0.5)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VisualHints {
    pub pre_capture: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InteractionCommand {
    Noop,
    SetPose { x: f64, y: f64 },
    SetVisual { pre_capture: bool },
    RequestFocus,
    SuppressRepulsion,
}
