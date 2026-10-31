fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rerun-if-changed=src/notifications.m");
        cc::Build::new()
            .file("src/notifications.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            // Matches the NSUserNotificationCenter backend used by the plugin.
            .flag("-Wno-deprecated-declarations")
            .compile("riimail_notifications");
        println!("cargo:rustc-link-lib=framework=AppKit");
    }
    tauri_build::build()
}
