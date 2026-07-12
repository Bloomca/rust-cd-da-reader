#[cfg(target_os = "macos")]
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    println!("cargo:rerun-if-changed=build.rs");
    const NATIVE_DIR: &str = "src/platform/macos/native";

    println!("cargo:rerun-if-changed={NATIVE_DIR}/shim_common.h");
    println!("cargo:rerun-if-changed={NATIVE_DIR}/device_service.c");
    println!("cargo:rerun-if-changed={NATIVE_DIR}/list_drives.c");
    println!("cargo:rerun-if-changed={NATIVE_DIR}/toc_reader.c");
    println!("cargo:rerun-if-changed={NATIVE_DIR}/track_information.c");
    println!("cargo:rerun-if-changed={NATIVE_DIR}/read_cd.c");

    println!("cargo:rustc-link-lib=framework=IOKit");
    println!("cargo:rustc-link-lib=framework=CoreFoundation");
    cc::Build::new()
        .file(format!("{NATIVE_DIR}/device_service.c"))
        .file(format!("{NATIVE_DIR}/list_drives.c"))
        .file(format!("{NATIVE_DIR}/toc_reader.c"))
        .file(format!("{NATIVE_DIR}/track_information.c"))
        .file(format!("{NATIVE_DIR}/read_cd.c"))
        .include(NATIVE_DIR)
        // force C compilation
        .flag("-x")
        .flag("c")
        .compile("macos_cd_shim");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
