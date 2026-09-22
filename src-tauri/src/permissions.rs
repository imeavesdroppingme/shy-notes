//! Platform permission helpers (macOS Accessibility).

#[cfg(target_os = "macos")]
mod macos {
    use macos_accessibility_client::accessibility::{
        application_is_trusted, application_is_trusted_with_prompt,
    };

    pub fn is_trusted(prompt: bool) -> bool {
        if prompt {
            application_is_trusted_with_prompt()
        } else {
            application_is_trusted()
        }
    }

    pub fn open_settings() {
        use std::process::Command;
        let _ = Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn();
    }

    pub fn current_exe_display() -> String {
        std::env::current_exe()
            .ok()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(unknown)".into())
    }
}

#[cfg(target_os = "macos")]
pub fn is_accessibility_trusted(prompt: bool) -> bool {
    macos::is_trusted(prompt)
}

#[cfg(target_os = "macos")]
pub fn open_accessibility_settings() {
    macos::open_settings();
}

#[cfg(target_os = "macos")]
pub fn current_exe_display() -> String {
    macos::current_exe_display()
}

#[cfg(not(target_os = "macos"))]
pub fn is_accessibility_trusted(_prompt: bool) -> bool {
    true
}

#[cfg(not(target_os = "macos"))]
pub fn open_accessibility_settings() {}

#[cfg(not(target_os = "macos"))]
pub fn current_exe_display() -> String {
    std::env::current_exe()
        .ok()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(unknown)".into())
}
