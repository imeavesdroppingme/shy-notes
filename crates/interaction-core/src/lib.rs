//! Pure geometric evasion / capture logic. No I/O, no window APIs.

mod geometry;
mod params;
mod state;
mod types;

pub use geometry::{
    aims_at_capture, capture_center, capture_miss_factor, clamp_pose_to_layout, is_pose_in_corner,
    point_in_circle, ray_hits_circle, rect_contains_rect, repulsion_delta, resolve_flee_pose,
    soft_away_from_point, swap_along_repulsion_ring,
};
pub use params::InteractionParams;
pub use state::{InteractionController, InteractionState};
pub use types::{
    DesktopLayout, InteractionCommand, MonitorInfo, MouseSample, VisualHints, WidgetPose,
};
