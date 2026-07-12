use std::path::PathBuf;

pub fn fresh_output_dir(example: &str) -> std::io::Result<PathBuf> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("example-output")
        .join(example);

    if directory.exists() {
        std::fs::remove_dir_all(&directory)?;
    }
    std::fs::create_dir_all(&directory)?;

    println!("Output directory: {}", directory.display());
    Ok(directory)
}
