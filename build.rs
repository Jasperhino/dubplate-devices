fn main() {
    assert_eq!(
        std::env::var("CARGO_CFG_TARGET_OS").unwrap(),
        "macos",
        "macOS only"
    );
    cc::Build::new()
        .file("native/macos.m")
        .flag("-fobjc-arc")
        .flag("-fblocks")
        .flag("-mmacosx-version-min=14.0")
        .compile("devices_native");
    for framework in ["AppKit", "ApplicationServices", "IOKit"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
    println!("cargo:rerun-if-changed=native/macos.m");
}
