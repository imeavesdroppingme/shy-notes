//! Platform permission helpers (macOS Accessibility).

#[cfg(target_os = "macos")]
pub fn is_accessibility_trusted(_prompt: bool) -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }
    unsafe { AXIsProcessTrusted() != 0 }
}

#[cfg(target_os = "macos")]
pub fn open_accessibility_settings() {
    use std::process::Command;
    // Opens System Settings to Privacy → Accessibility on modern macOS.
    let _ = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

#[cfg(not(target_os = "macos"))]
pub fn is_accessibility_trusted(_prompt: bool) -> bool {
    true
}

#[cfg(not(target_os = "macos"))]
pub fn open_accessibility_settings() {}
