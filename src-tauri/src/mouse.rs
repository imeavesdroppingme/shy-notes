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
        if self.device.is_none() {
            return None;
        }
        let (x, y) = cursor_coords(self.device.as_ref()?)?;
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
        if self.device.is_none() {
            return false;
        }
        ctrl_is_down(self.device.as_ref())
    }
}

#[cfg(windows)]
fn cursor_coords(_device: &DeviceState) -> Option<(i32, i32)> {
    // GetCursorPos only — device_query also polls five mouse buttons every call.
    #[link(name = "user32")]
    extern "system" {
        fn GetCursorPos(lp_point: *mut Point) -> i32;
    }
    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }
    let mut pt = Point { x: 0, y: 0 };
    let ok = unsafe { GetCursorPos(&mut pt) };
    if ok != 0 {
        Some((pt.x, pt.y))
    } else {
        None
    }
}

#[cfg(not(windows))]
fn cursor_coords(device: &DeviceState) -> Option<(i32, i32)> {
    let (x, y) = device.get_mouse().coords;
    Some((x, y))
}

#[cfg(windows)]
fn ctrl_is_down(_device: Option<&DeviceState>) -> bool {
    // device_query scans GetAsyncKeyState for all 256 virtual keys every call —
    // ~15k syscalls/s at 60Hz. We only need left/right Control.
    #[link(name = "user32")]
    extern "system" {
        fn GetAsyncKeyState(v_key: i32) -> i16;
    }
    const VK_LCONTROL: i32 = 0xA2;
    const VK_RCONTROL: i32 = 0xA3;
    unsafe {
        (GetAsyncKeyState(VK_LCONTROL) as u16 & 0x8000) != 0
            || (GetAsyncKeyState(VK_RCONTROL) as u16 & 0x8000) != 0
    }
}

#[cfg(not(windows))]
fn ctrl_is_down(device: Option<&DeviceState>) -> bool {
    let Some(device) = device else {
        return false;
    };
    let keys = device.get_keys();
    keys.contains(&Keycode::LControl) || keys.contains(&Keycode::RControl)
}
