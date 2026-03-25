/// Reads the first audio track with an aggressive retry configuration
/// suitable for scratched or damaged discs.
/// By default, it already retries multiple times with smaller number
/// of sectors, so this usually should not be necessary, but you can see
/// here that you can tweak details.
use cd_da_reader::{CdReader, RetryConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let reader = CdReader::open_default()?;
    let toc = reader.read_toc()?;

    let first_audio = toc
        .tracks
        .iter()
        .find(|t| t.is_audio)
        .ok_or("no audio tracks found")?;

    // More attempts, longer backoff, and sector reduction down to 1
    // for maximum resilience on scratched media.
    let retry = RetryConfig {
        max_attempts: 8,
        initial_backoff_ms: 50,
        max_backoff_ms: 1000,
        reduce_chunk_on_retry: true,
        min_sectors_per_read: 1,
    };

    println!(
        "Reading track {} with aggressive retry...",
        first_audio.number
    );
    let data = reader.read_track_with_retry(&toc, first_audio.number, &retry)?;

    let wav = CdReader::create_wav(data);
    let filename = format!("track{:02}.wav", first_audio.number);
    std::fs::write(&filename, wav)?;
    println!("Saved {}", filename);

    Ok(())
}
