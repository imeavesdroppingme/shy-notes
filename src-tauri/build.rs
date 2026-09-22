fn main() {
    tauri_build::build();
    // Prefer an explicit CI date; otherwise stamp UTC calendar day at compile time.
    let build_date = std::env::var("SHY_NOTES_BUILD_DATE").unwrap_or_else(|_| {
        chrono::Utc::now().format("%Y-%m-%d").to_string()
    });
    println!("cargo:rustc-env=SHY_NOTES_BUILD_DATE={build_date}");
    println!("cargo:rerun-if-env-changed=SHY_NOTES_BUILD_DATE");
}
