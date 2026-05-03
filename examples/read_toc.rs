/// Opens the default CD drive and prints the Table of Contents.
use cd_da_reader::CdReader;

const CD_EXTRA_TRAILING_DATA_GAP_SECTORS: u32 = 11_400;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let reader = CdReader::open_default()?;
    let toc = reader.read_toc()?;

    println!("Table of Contents\n");

    println!(
        "Tracks {}-{} ({} total), lead-out at LBA {}\n",
        toc.first_track,
        toc.last_track,
        toc.tracks.len(),
        toc.leadout_lba,
    );

    for track in &toc.tracks {
        let kind = if track.is_audio { "audio" } else { "data " };
        let (m, s, f) = track.start_msf;
        let sectors = track_end_lba(&toc, track.number) - track.start_lba;
        let duration_secs = sectors as f64 / 75.0;
        let mins = (duration_secs / 60.0) as u32;
        let secs = (duration_secs % 60.0) as u32;

        println!(
            "  #{:>2}  {}  LBA {:>6}  MSF {:02}:{:02}.{:02}  duration: {:02}:{:02}",
            track.number, kind, track.start_lba, m, s, f, mins, secs,
        );
    }

    Ok(())
}

/// Returns the exclusive end LBA for a track.
fn track_end_lba(toc: &cd_da_reader::Toc, track_no: u8) -> u32 {
    let idx = toc
        .tracks
        .iter()
        .position(|t| t.number == track_no)
        .unwrap();

    // in case all next tracks are data (but they do exist),
    // we need to subtract 11,400 sectors
    if toc.tracks[idx].is_audio
        && idx + 1 < toc.tracks.len()
        && toc.tracks[idx + 1..].iter().all(|track| !track.is_audio)
    {
        return toc.tracks[idx + 1]
            .start_lba
            .saturating_sub(CD_EXTRA_TRAILING_DATA_GAP_SECTORS);
    }

    if idx + 1 < toc.tracks.len() {
        toc.tracks[idx + 1].start_lba
    } else {
        toc.leadout_lba
    }
}
