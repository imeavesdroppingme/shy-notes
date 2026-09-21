//! Global mouse sampling.

use device_query::{DeviceQuery, DeviceState};
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
}
