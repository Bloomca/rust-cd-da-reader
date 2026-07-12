/// Reads the first data track from a mixed-mode / enhanced CD and verifies the
/// result against the on-disc structure, so it doubles as a correctness check
/// for the cooked (2048 B) and raw (2352 B) data read paths.
///
/// What it checks, using only the data the disc itself carries:
///   1. Raw sector framing: a 2352 B sector starts with the 12-byte sync
///      pattern `00 FF*10 00`, and byte 15 reports the sector mode.
///   2. ISO 9660 signature: logical sector 16 of the volume is the Primary
///      Volume Descriptor — type byte `0x01` followed by `"CD001"`.
///   3. Cooked vs raw: the cooked 2048 B must equal the user-data region of
///      the raw sector (offset 16 for Mode 1).
use cd_da_reader::{CdReader, ReadOptions, SectorReadFormat};

// ISO 9660 places the Primary Volume Descriptor at logical sector 16.
const PVD_SECTOR_OFFSET: u32 = 16;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let reader = CdReader::open_default()?;
    let toc = reader.read_toc()?;

    let data_track = toc
        .tracks
        .iter()
        .find(|t| !t.is_audio)
        .ok_or("no data track on this disc (need a mixed-mode / enhanced CD)")?;

    let pvd_lba = data_track.start_lba + PVD_SECTOR_OFFSET;
    println!(
        "Data track #{} starts at LBA {}; reading PVD at LBA {}\n",
        data_track.number, data_track.start_lba, pvd_lba
    );

    let mut options = ReadOptions::default().with_format(SectorReadFormat::Mode1Raw);

    // --- raw read (2352 B) -------------------------------------------------
    let raw = reader.read_sector_range(pvd_lba, 1, &options)?;
    if raw.len() != 2352 {
        return Err(format!("raw read returned {} bytes, expected 2352", raw.len()).into());
    }

    let sync_ok = raw[0] == 0x00 && raw[1..11].iter().all(|&b| b == 0xFF) && raw[11] == 0x00;
    let mode = raw[15];
    println!("raw sync pattern : {}", pass(sync_ok));
    println!("raw sector mode  : Mode {mode}");

    // User data sits after sync(12) + header(4) for Mode 1, and additionally
    // after an 8-byte subheader for Mode 2 Form 1.
    let user_offset = match mode {
        1 => 16,
        2 => 24,
        other => return Err(format!("unexpected sector mode {other}").into()),
    };
    let raw_user = &raw[user_offset..user_offset + 2048];

    let iso_ok = raw_user[0] == 0x01 && &raw_user[1..6] == b"CD001";
    println!("ISO 9660 'CD001' : {}", pass(iso_ok));

    // --- cooked read (2048 B) ---------------------------------------------
    // The cooked format is specifically Mode 1, so only cross-check it there.
    if mode == 1 {
        options = options.with_format(SectorReadFormat::Mode1Cooked);
        let cooked = reader.read_sector_range(pvd_lba, 1, &options)?;
        if cooked.len() != 2048 {
            return Err(
                format!("cooked read returned {} bytes, expected 2048", cooked.len()).into(),
            );
        }
        let matches_raw = cooked == raw_user;
        println!("cooked == raw[16..]: {}", pass(matches_raw));

        if sync_ok && iso_ok && matches_raw {
            println!("\nALL CHECKS PASSED — cooked and raw data reads are correct.");
            return Ok(());
        }
    } else {
        println!(
            "\nData track is Mode {mode} (e.g. CD-Extra / Mode 2 Form 1). The cooked path \
             targets Mode 1, so only the raw checks apply here."
        );
        if sync_ok && iso_ok {
            println!("Raw read verified against on-disc ISO structure.");
            return Ok(());
        }
    }

    Err("one or more verification checks FAILED — see output above".into())
}

fn pass(ok: bool) -> &'static str {
    if ok { "PASS" } else { "FAIL" }
}
