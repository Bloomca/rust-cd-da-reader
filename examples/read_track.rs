use cd_da_reader::CdReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let drive_path = default_drive_path();
    read_cd(drive_path)
}

#[cfg(target_os = "windows")]
fn default_drive_path() -> &'static str {
    r"\\.\E:"
}

#[cfg(target_os = "macos")]
fn default_drive_path() -> &'static str {
    "disk14"
}

#[cfg(target_os = "linux")]
fn default_drive_path() -> &'static str {
    "/dev/sr0"
}

fn read_cd(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let reader = CdReader::open(path)?;
    let toc = reader.read_toc()?;
    println!("{:#?}", toc);

    let last_audio_track = toc
        .tracks
        .iter()
        .rev()
        .find(|track| track.is_audio)
        .ok_or_else(|| std::io::Error::other("no audio tracks in TOC"))?;

    println!("Reading track {}", last_audio_track.number);
    let data = reader.read_track(&toc, last_audio_track.number)?;
    let wav_track = CdReader::create_wav(data);
    std::fs::write("myfile.wav", wav_track)?;

    Ok(())
}
