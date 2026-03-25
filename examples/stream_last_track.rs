/// Reads the last audio track using the streaming API and saves it as a WAV file.
use cd_da_reader::{CdReader, TrackStreamConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let reader = CdReader::open_default()?;
    let toc = reader.read_toc()?;

    let last_audio = toc
        .tracks
        .iter()
        .rev()
        .find(|t| t.is_audio)
        .ok_or("no audio tracks found")?;

    println!("Streaming track {}...", last_audio.number);
    let mut stream =
        reader.open_track_stream(&toc, last_audio.number, TrackStreamConfig::default())?;

    let mut pcm = Vec::new();
    while let Some(chunk) = stream.next_chunk()? {
        pcm.extend_from_slice(&chunk);
    }

    let wav = CdReader::create_wav(pcm);
    let filename = format!("track{:02}.wav", last_audio.number);
    std::fs::write(&filename, wav)?;
    println!("Saved {}", filename);

    Ok(())
}
