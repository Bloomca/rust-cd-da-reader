/// Reads the first audio track with an aggressive retry configuration
/// suitable for scratched or damaged discs.
/// By default, it already retries multiple times with smaller number
/// of sectors, so this usually should not be necessary, but you can see
/// here that you can tweak details.
mod common;

use std::time::Duration;

use cd_da_reader::{CdReader, ReadOptions, RetryConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = common::fresh_output_dir("custom_retry")?;
    let reader = CdReader::open_default()?;
    let toc = reader.read_toc()?;

    let first_audio = toc
        .tracks
        .iter()
        .find(|t| t.is_audio)
        .ok_or("no audio tracks found")?;

    // More attempts, longer backoff, and sector reduction down to 1
    // for maximum resilience on scratched media.
    let retry = RetryConfig::default()
        .with_max_attempts(8)
        .with_initial_backoff(Duration::from_millis(50))
        .with_max_backoff(Duration::from_secs(1))
        .with_chunk_reduction(true)
        .with_min_sectors_per_read(1);
    let options = ReadOptions::default().with_retry(retry);

    println!(
        "Reading track {} with aggressive retry...",
        first_audio.number
    );
    let data = reader.read_track_with_options(&toc, first_audio.number, &options)?;

    let wav = CdReader::create_wav(data);
    let output_path = output_dir.join(format!("track{:02}.wav", first_audio.number));
    std::fs::write(&output_path, wav)?;
    println!("Saved {}", output_path.display());

    Ok(())
}
