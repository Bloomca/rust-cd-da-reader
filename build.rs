#[cfg(target_os = "macos")]
fn main() {
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
