//! Reads a short preview of the first audio track from the default drive and
//! plays it out loud, so you can confirm by ear that audio extraction works —
//! including on a mixed-mode / enhanced CD where data tracks sit alongside the
//! audio ones.
//!
//! It reads only the first N seconds (30 by default) rather than the whole
//! track, so it returns quickly. CD-DA is 75 sectors/second and already raw PCM
//! (44100 Hz, 16-bit signed little-endian, stereo), which is exactly WAV's
//! native format, so `create_wav` just prepends a 44-byte RIFF header and the
//! result is directly playable — no codecs, no extra crates.
//!
//! Usage:
//!   cargo run --example play_audio_track            # first 30 seconds
//!   cargo run --example play_audio_track -- 60      # first 60 seconds
//!
//! The WAV is written to the current directory and then handed to the platform's
//! built-in player (`afplay` on macOS, `Media.SoundPlayer` on Windows). On other
//! platforms it is saved and you are told how to play it yourself.
use cd_da_reader::{CdReader, RetryConfig, SectorReadMode};

/// CD-DA plays 75 sectors (each 2352 bytes) per second.
const SECTORS_PER_SECOND: u32 = 75;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let seconds: u32 = match std::env::args().nth(1) {
        Some(a) => a.parse()?,
        None => 30,
    };

    let reader = CdReader::open_default()?;
    let toc = reader.read_toc()?;

    let track = toc
        .tracks
        .iter()
        .find(|t| t.is_audio)
        .ok_or("no audio tracks found on this disc")?;

    // Clamp the preview to what the track actually holds: the track ends where
    // the next track starts, or at the lead-out if it's the last one.
    let track_end = toc
        .tracks
        .iter()
        .map(|t| t.start_lba)
        .filter(|&lba| lba > track.start_lba)
        .min()
        .unwrap_or(toc.leadout_lba);
    let track_sectors = track_end - track.start_lba;
    let sectors = (seconds * SECTORS_PER_SECOND).min(track_sectors);
    let actual_seconds = sectors / SECTORS_PER_SECOND;

    println!(
        "Reading first {actual_seconds}s ({sectors} sectors) of audio track #{}...",
        track.number
    );
    let pcm = reader.read_data_sectors(
        track.start_lba,
        sectors,
        SectorReadMode::Audio,
        &RetryConfig::default(),
    )?;
    println!(
        "Read {} bytes of PCM ({:.1} MiB)",
        pcm.len(),
        pcm.len() as f64 / (1024.0 * 1024.0)
    );

    let filename = format!("track{:02}_preview.wav", track.number);
    std::fs::write(&filename, CdReader::create_wav(pcm))?;
    println!("Saved {filename}");

    play(&filename)
}

/// Hand the WAV to the OS's built-in player and block until it finishes.
#[cfg(target_os = "macos")]
fn play(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Playing (Ctrl+C to stop)...");
    let status = std::process::Command::new("afplay").arg(path).status()?;
    if !status.success() {
        return Err("afplay exited with an error".into());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn play(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Playing (this blocks until the preview ends)...");
    // SoundPlayer.PlaySync plays a WAV synchronously using the built-in player.
    let script = format!("(New-Object Media.SoundPlayer '{path}').PlaySync()");
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .status()?;
    if !status.success() {
        return Err("powershell SoundPlayer exited with an error".into());
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn play(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Saved the WAV — play it with your audio player, e.g. `aplay {path}`.");
    Ok(())
}
