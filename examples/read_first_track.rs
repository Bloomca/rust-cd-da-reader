/// Reads the first audio track from the default CD drive and saves it as a WAV file.
mod common;

use cd_da_reader::CdReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = common::fresh_output_dir("read_first_track")?;
    let reader = CdReader::open_default()?;
    let toc = reader.read_toc()?;

    let first_audio = toc
        .tracks
        .iter()
        .find(|t| t.is_audio)
        .ok_or("no audio tracks found")?;

    println!("Reading track {}...", first_audio.number);
    let data = reader.read_track(&toc, first_audio.number)?;

    let wav = CdReader::create_wav(data);
    let output_path = output_dir.join(format!("track{:02}.wav", first_audio.number));
    std::fs::write(&output_path, wav)?;
    println!("Saved {}", output_path.display());

    Ok(())
}
