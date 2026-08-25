/// Reads the first audio track repeatedly at different requested speeds and reports the
/// elapsed time for each read.
use std::time::Instant;

use cd_da_reader::{CdReader, ReadOptions, ReadSpeed};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let reader = CdReader::open_default()?;
    let toc = reader.read_toc()?;

    let first_audio = toc
        .tracks
        .iter()
        .find(|track| track.is_audio)
        .ok_or("no audio tracks found")?;

    // Keep "unchanged" immediately after 1x so it tests whether the previous speed
    // request remains in effect when no new speed command is sent.
    let speed_tests = [
        ("1x", ReadSpeed::CustomMultiplier(1)),
        ("unchanged (after 1x)", ReadSpeed::Unchanged),
        ("10x", ReadSpeed::CustomMultiplier(10)),
        ("30x", ReadSpeed::CustomMultiplier(30)),
        ("optimal", ReadSpeed::Optimal),
    ];

    println!(
        "Reading audio track {} {} times\n",
        first_audio.number,
        speed_tests.len()
    );

    let mut timings = Vec::with_capacity(speed_tests.len());

    for (label, read_speed) in speed_tests {
        let options = ReadOptions::default().with_read_speed(read_speed);

        println!("Reading at {label}...");
        let started = Instant::now();
        let data = reader.read_track_with_options(&toc, first_audio.number, &options)?;
        let elapsed = started.elapsed();

        println!(
            "Read {} bytes in {:.3} seconds\n",
            data.len(),
            elapsed.as_secs_f64()
        );
        timings.push((label, elapsed));
    }

    println!("Timing summary:");
    for (label, elapsed) in timings {
        println!("  {label:<22} {:>10.3} seconds", elapsed.as_secs_f64());
    }

    Ok(())
}
