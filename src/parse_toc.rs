use crate::{Toc, Track};

pub fn parse_toc(data: Vec<u8>) -> std::io::Result<Toc> {
    // TOC data format:
    // Bytes 0-1: TOC data length
    // Byte 2: First track number
    // Byte 3: Last track number
    // Bytes 4+: Track descriptors (8 bytes each)

    if data.len() < 4 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "TOC data too short"));
    }

    let toc_length = u16::from_be_bytes([data[0], data[1]]) as usize;
    let first_track = data[2];
    let last_track = data[3];

    let mut tracks = vec![];
    let mut offset = 4;

    while offset + 8 <= data.len() && offset < toc_length + 2 {
        let track_num = data[offset + 2];
        let control = data[offset + 1];

        // LBA is in bytes 4-7 of descriptor
        let lba = u32::from_be_bytes([
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]);

        let msf = lba_to_msf(lba);

        // Skip lead-out track (0xAA) for now
        if track_num != 0xAA {
            tracks.push(Track {
                number: track_num,
                start_lba: lba,
                start_msf: msf,
                is_audio: (control & 0x04) == 0,
            });
        }
        
        offset += 8;
    }

    Ok(Toc {
        first_track,
        last_track,
        tracks,
    })
}


fn lba_to_msf(lba: u32) -> (u8, u8, u8) {
    let total_frames = lba + 150;  // MSF addresses are offset by 150
    let minutes = (total_frames / 75 / 60) as u8;
    let seconds = ((total_frames / 75) % 60) as u8;
    let frames = (total_frames % 75) as u8;
    (minutes, seconds, frames)
}
