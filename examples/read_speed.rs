/// Read the first audio track at 10x speed, and read the second audio track at `Optimal`
/// speed.
mod common;

use cd_da_reader::{CdReader, ReadOptions, ReadSpeed, Track};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = common::fresh_output_dir("read_speed")?;
    let reader = CdReader::open_default()?;
    let toc = reader.read_toc()?;

    let audio_tracks: Vec<&Track> = toc.tracks.iter().filter(|t| t.is_audio).collect();

    if audio_tracks.len() < 2 {
        panic!("This example requires at least two audio tracks");
    }

    let first_track = audio_tracks[0];
    let second_track = audio_tracks[1];

    // Read the first track with 10x speed
    {
        let options_10x = ReadOptions::default().with_read_speed(ReadSpeed::CustomMultiplier(10));

        println!("Reading track {} with 10x speed...", first_track.number);
        let data = reader.read_track_with_options(&toc, first_track.number, &options_10x)?;

        let wav = CdReader::create_wav(data);
        let output_path = output_dir.join(format!("track{:02}.wav", first_track.number));
        std::fs::write(&output_path, wav)?;
        println!("Saved {}", output_path.display());
    }

    // Read the second track with "optimal" speed
    {
        let options_optimal = ReadOptions::default().with_read_speed(ReadSpeed::Optimal);
        println!(
            "Reading track {} with optimal speed...",
            second_track.number
        );
        let data = reader.read_track_with_options(&toc, second_track.number, &options_optimal)?;

        let wav = CdReader::create_wav(data);
        let output_path = output_dir.join(format!("track{:02}.wav", second_track.number));
        std::fs::write(&output_path, wav)?;
        println!("Saved {}", output_path.display());
    }

    Ok(())
}
