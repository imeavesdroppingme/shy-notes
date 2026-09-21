//! Tunable interaction parameters (centralized).

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteractionParams {
    pub capture_diameter: f64,
    pub influence_radius: f64,
    pub pre_capture_delay_ms: u64,
    pub look_ahead_px: f64,
    pub still_speed_threshold: f64,
    pub repulsion_strength: f64,
    pub repulsion_max_step: f64,
    pub animation_lerp: f64,
}

impl Default for InteractionParams {
    fn default() -> Self {
        Self {
            capture_diameter: 100.0,
            // Large influence so a flee crosses most of the display.
            influence_radius: 900.0,
            pre_capture_delay_ms: 400,
            look_ahead_px: 150.0,
            still_speed_threshold: 12.0,
            repulsion_strength: 2.6,
            // Peak per-tick jump at closest approach (distance falloff scales it down).
            repulsion_max_step: 187.0,
            animation_lerp: 1.0,
        }
    }
}
