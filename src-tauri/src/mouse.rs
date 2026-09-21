//! Global mouse sampling and modifier keys.

use device_query::{DeviceQuery, DeviceState, Keycode};
use interaction_core::MouseSample;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct MouseTracker {
    device: DeviceState,
}

impl MouseTracker {
    pub fn new() -> Self {
        Self {
            device: DeviceState::new(),
        }
    }

    pub fn sample(&self) -> MouseSample {
        let (x, y) = self.device.get_mouse().coords;
        let t_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        MouseSample {
            t_ms,
            x_phys: x as f64,
            y_phys: y as f64,
        }
    }

    /// True while either Control key is held (suppresses repulsion).
    pub fn ctrl_held(&self) -> bool {
        let keys = self.device.get_keys();
        keys.contains(&Keycode::LControl) || keys.contains(&Keycode::RControl)
    }
}
