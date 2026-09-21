//! Local persistence for text and window pose.

use interaction_core::WidgetPose;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStateSnapshot {
    pub version: u32,
    pub text: String,
    pub pose: PoseDto,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoseDto {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Default for AppStateSnapshot {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            text: String::new(),
            pose: PoseDto {
                x: 80.0,
                y: 80.0,
                w: 320.0,
                h: 280.0,
            },
            pinned: false,
        }
    }
}

impl AppStateSnapshot {
    pub fn to_pose(&self) -> WidgetPose {
        WidgetPose {
            x: self.pose.x,
            y: self.pose.y,
            w: self.pose.w,
            h: self.pose.h,
            pinned: self.pinned,
        }
    }
}

pub fn state_path() -> PathBuf {
    let base = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("shy-notes");
    let _ = fs::create_dir_all(&base);
    base.join("state.json")
}

pub fn load_state() -> AppStateSnapshot {
    let path = state_path();
    match fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => AppStateSnapshot::default(),
    }
}

pub fn save_state(state: &AppStateSnapshot) -> Result<(), String> {
    let path = state_path();
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(())
}
