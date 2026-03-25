/// Reads the first audio track from the default CD drive and saves it as a WAV file.
use cd_da_reader::CdReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let filename = format!("track{:02}.wav", first_audio.number);
    std::fs::write(&filename, wav)?;
    println!("Saved {}", filename);

    Ok(())
}
