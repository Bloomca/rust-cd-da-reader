#[cfg(target_os = "macos")]
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/mac/shim_common.h");
    println!("cargo:rerun-if-changed=src/mac/da_guard.c");
    println!("cargo:rerun-if-changed=src/mac/device_service.c");
    println!("cargo:rerun-if-changed=src/mac/toc_reader.c");
    println!("cargo:rerun-if-changed=src/mac/audio_reader.c");

    println!("cargo:rustc-link-lib=framework=IOKit");
    println!("cargo:rustc-link-lib=framework=CoreFoundation");
    println!("cargo:rustc-link-lib=framework=DiskArbitration");
    cc::Build::new()
        .file("src/mac/da_guard.c")
        .file("src/mac/device_service.c")
        .file("src/mac/toc_reader.c")
        .file("src/mac/audio_reader.c")
        .include("src/mac")
        // force C compilation
        .flag("-x")
        .flag("c")
        .compile("macos_cd_shim");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
