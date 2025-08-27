fn main() {
    println!("cargo:rustc-link-lib=framework=IOKit");
    println!("cargo:rustc-link-lib=framework=CoreFoundation");
    println!("cargo:rustc-link-lib=framework=DiskArbitration");
    cc::Build::new()
        .file("src/macos_cd_shim.c")
        .compile("macos_cd_shim");
}
