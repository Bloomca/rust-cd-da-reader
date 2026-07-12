/// Reads the last audio track using the streaming API and saves it as a WAV file.
mod common;

use cd_da_reader::CdReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = common::fresh_output_dir("stream_last_track")?;
    let reader = CdReader::open_default()?;
    let toc = reader.read_toc()?;

    let last_audio = toc
        .tracks
        .iter()
        .rev()
        .find(|t| t.is_audio)
        .ok_or("no audio tracks found")?;

    println!("Streaming track {}...", last_audio.number);
    let mut stream = reader.open_track_stream(&toc, last_audio.number)?;

    let mut pcm = Vec::new();
    while let Some(chunk) = stream.next_chunk()? {
        pcm.extend_from_slice(&chunk);
    }

    let wav = CdReader::create_wav(pcm);
    let output_path = output_dir.join(format!("track{:02}.wav", last_audio.number));
    std::fs::write(&output_path, wav)?;
    println!("Saved {}", output_path.display());

    Ok(())
}
