use cd_da_reader::{CdReader, RetryConfig, TrackStreamConfig};

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
    println!("{toc:#?}");

    let last_audio_track = toc
        .tracks
        .iter()
        .rev()
        .find(|track| track.is_audio)
        .ok_or_else(|| std::io::Error::other("no audio tracks in TOC"))?;

    println!("Reading track {}", last_audio_track.number);
    let stream_cfg = TrackStreamConfig {
        sectors_per_chunk: 27,
        retry: RetryConfig {
            max_attempts: 5,
            initial_backoff_ms: 30,
            max_backoff_ms: 500,
            reduce_chunk_on_retry: true,
            min_sectors_per_read: 1,
        },
    };
    let mut stream = reader.open_track_stream(&toc, last_audio_track.number, stream_cfg)?;

    let mut pcm = Vec::new();
    while let Some(chunk) = stream.next_chunk()? {
        pcm.extend_from_slice(&chunk);
    }
    let wav = CdReader::create_wav(pcm);
    std::fs::write("myfile.wav", wav)?;

    Ok(())
}
