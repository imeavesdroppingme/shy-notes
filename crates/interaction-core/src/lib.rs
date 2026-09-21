//! Pure geometric evasion / capture logic. No I/O, no window APIs.

mod geometry;
mod params;
mod state;
mod types;

pub use geometry::{
    capture_center, clamp_pose_to_layout, point_in_circle, ray_hits_circle, rect_contains_rect,
    repulsion_delta, soft_away_from_point,
};
pub use params::InteractionParams;
pub use state::{InteractionController, InteractionState};
pub use types::{
    DesktopLayout, InteractionCommand, MonitorInfo, MouseSample, VisualHints, WidgetPose,
};
