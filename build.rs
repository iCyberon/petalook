fn main() {
    assert!(
        std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos"),
        "Petalook requires macOS"
    );
    let sparkle = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/sparkle/2.10.0");
    assert!(
        sparkle.join("Sparkle.framework/Headers/Sparkle.h").exists(),
        "Run bash scripts/fetch-sparkle.sh before building"
    );
    cc::Build::new()
        .file("native/media.m")
        .file("native/window.m")
        .file("native/updater.m")
        .flag(format!("-F{}", sparkle.display()))
        .flag("-fobjc-arc")
        .flag("-Wno-deprecated-declarations")
        .compile("petalook_native");
    for framework in [
        "AppKit",
        "AVFoundation",
        "CoreMedia",
        "CoreImage",
        "CoreVideo",
        "ImageIO",
        "QuartzCore",
        "UniformTypeIdentifiers",
    ] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
    println!("cargo:rerun-if-changed=native/media.m");
    println!("cargo:rerun-if-changed=native/window.m");
    println!("cargo:rerun-if-changed=native/slide_geometry.h");
    println!("cargo:rerun-if-changed=native/updater.m");
}
