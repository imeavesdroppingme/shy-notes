//! Global mouse sampling and modifier keys.

use device_query::{DeviceQuery, DeviceState, Keycode};
use interaction_core::MouseSample;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct MouseTracker {
    /// Lazily created — `DeviceState::new()` panics on macOS without Accessibility.
    device: Option<DeviceState>,
}

impl MouseTracker {
    pub fn new() -> Self {
        Self { device: None }
    }

    /// Create the native mouse backend when permissions/display allow it.
    /// Never panics.
    pub fn ensure_ready(&mut self) -> bool {
        if self.device.is_some() {
            return true;
        }

        #[cfg(target_os = "macos")]
        {
            // Do not call DeviceState::checked_new() in a polling loop: it prompts
            // via AXIsProcessTrustedWithOptions on every call. Trust is checked by
            // the adapter; only then construct (catch_unwind if TCC races).
            if !crate::permissions::is_accessibility_trusted(false) {
                return false;
            }
            match std::panic::catch_unwind(DeviceState::new) {
                Ok(device) => {
                    self.device = Some(device);
                    true
                }
                Err(_) => false,
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            match DeviceState::checked_new() {
                Some(device) => {
                    self.device = Some(device);
                    true
                }
                None => false,
            }
        }
    }

    pub fn sample(&self) -> Option<MouseSample> {
        let device = self.device.as_ref()?;
        let (x, y) = device.get_mouse().coords;
        let t_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Some(MouseSample {
            t_ms,
            x_phys: x as f64,
            y_phys: y as f64,
        })
    }

    /// True while either Control key is held (suppresses repulsion).
    pub fn ctrl_held(&self) -> bool {
        let Some(device) = self.device.as_ref() else {
            return false;
        };
        let keys = device.get_keys();
        keys.contains(&Keycode::LControl) || keys.contains(&Keycode::RControl)
    }
}
