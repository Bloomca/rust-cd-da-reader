use crate::Toc;

pub fn get_track_bounds(toc: &Toc, track_no: u8) -> std::io::Result<(u32, u32)> {
    let idx = toc
        .tracks
        .iter()
        .position(|t| t.number == track_no)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "track not in TOC"))?;

    let start_lba = toc.tracks[idx].start_lba as u32;

    // Determine end LBA (next track start, or lead-out for the last track)
    let end_lba: u32 = if (idx + 1) < toc.tracks.len() {
        toc.tracks[idx + 1].start_lba as u32
    } else {
        toc.leadout_lba
    };

    if end_lba <= start_lba {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "bad TOC bounds",
        ));
    }

    let sectors = end_lba - start_lba;

    Ok((start_lba, sectors))
}
