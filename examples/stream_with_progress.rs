/// Streams the first audio track while printing a live progress line.
use cd_da_reader::{CdReader, TrackStreamConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let reader = CdReader::open_default()?;
    let toc = reader.read_toc()?;

    let first_audio = toc
        .tracks
        .iter()
        .find(|t| t.is_audio)
        .ok_or("no audio tracks found")?;

    let mut stream =
        reader.open_track_stream(&toc, first_audio.number, TrackStreamConfig::default())?;

    let total_secs = stream.total_seconds();
    println!(
        "Track {} — {} sectors ({:.0}s)\n",
        first_audio.number,
        stream.total_sectors(),
        total_secs,
    );

    let mut pcm = Vec::new();
    while let Some(chunk) = stream.next_chunk()? {
        pcm.extend_from_slice(&chunk);

        let cur = stream.current_seconds();
        let pct = cur / total_secs * 100.0;
        eprint!("\r  [{:>5.1}s / {:.1}s] {:5.1}%", cur, total_secs, pct,);
    }
    eprintln!("\r  [{:.1}s / {:.1}s] 100.0%", total_secs, total_secs);

    let wav = CdReader::create_wav(pcm);
    let filename = format!("track{:02}.wav", first_audio.number);
    std::fs::write(&filename, wav)?;
    println!("\nSaved {}", filename);

    Ok(())
}
