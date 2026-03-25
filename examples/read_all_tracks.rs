/// Reads every audio track from the default CD drive and saves each as a WAV file.
use cd_da_reader::CdReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let reader = CdReader::open_default()?;
    let toc = reader.read_toc()?;

    let audio_tracks: Vec<_> = toc.tracks.iter().filter(|t| t.is_audio).collect();
    println!("Found {} audio track(s)\n", audio_tracks.len());

    let mut failed = Vec::new();

    for track in &audio_tracks {
        print!("Reading track {:>2}... ", track.number);
        match reader.read_track(&toc, track.number) {
            Ok(data) => {
                let wav = CdReader::create_wav(data);
                let filename = format!("track{:02}.wav", track.number);
                std::fs::write(&filename, wav)?;
                println!("saved {}", filename);
            }
            Err(e) => {
                println!("FAILED: {}", e);
                failed.push(track.number);
            }
        }
    }

    if !failed.is_empty() {
        eprintln!("\nFailed to read tracks: {:?}", failed);
    }

    Ok(())
}
