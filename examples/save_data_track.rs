//! Save the data track of a mixed-mode / enhanced ("CD-Extra") disc as a
//! mountable image, then print how to mount it.
//!
//! This is the end-to-end data-track workflow:
//!   1. read the TOC and pick the (first) data track,
//!   2. auto-detect its sector format with `detect_track_format`,
//!   3. for Mode 1, stream it **cooked** (2048 B/sector) straight to a `.iso` —
//!      cooked Mode 1 is exactly the ISO 9660 filesystem image, so it mounts as
//!      is. Streaming keeps memory flat regardless of track size (a full data
//!      track can be hundreds of MB),
//!   4. Mode 2 is detected but not auto-cooked here (see the note it prints and
//!      `docs/consuming-cd-da-reader.md`).
//!
//! Reads and streams run over the same options and read path, so the only
//! difference from a blocking `read_track_with_options` is that we pull chunks
//! and write them as they arrive.
//!
//! Run with: `cargo run --example save_data_track`
mod common;

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use cd_da_reader::{CdReader, SectorReadFormat, Toc, TrackStreamOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = common::fresh_output_dir("save_data_track")?;
    let reader = CdReader::open_default()?;
    let toc = reader.read_toc()?;

    // There is no `find_data_track` helper in the crate — the idiom is a plain
    // filter on the TOC, since "data track" is simply `!is_audio`.
    let data_track = toc
        .tracks
        .iter()
        .find(|track| !track.is_audio)
        .ok_or("no data track on this disc (need a mixed-mode / enhanced CD)")?;

    let format = reader.detect_track_format(data_track)?;
    println!("Data track #{} detected as {format:?}\n", data_track.number);

    match format {
        SectorReadFormat::Mode1Cooked => {
            // Cooked Mode 1 strips sync/header/EDC/ECC, leaving exactly the
            // 2048-byte user data per sector — i.e. the raw ISO 9660 image.
            let iso_path = output_dir.join(format!("track{:02}.iso", data_track.number));
            let bytes = stream_track_to_file(&reader, &toc, data_track.number, format, &iso_path)?;

            println!(
                "Wrote {} ({bytes} bytes, {} sectors)\n",
                iso_path.display(),
                bytes / format.sector_size() as u64
            );
            print_mount_hint(&iso_path.display().to_string());
        }
        SectorReadFormat::Mode2Raw => {
            // Mode 2 forms are a per-sector property; producing a clean cooked
            // payload requires inspecting each sector's XA subheader, which is
            // left to the consumer. We save the complete raw sectors so nothing
            // is lost, and point at the docs.
            let bin_path = output_dir.join(format!("track{:02}.mode2.bin", data_track.number));
            let bytes = stream_track_to_file(&reader, &toc, data_track.number, format, &bin_path)?;

            println!(
                "This is a Mode 2 track. Saved complete raw sectors to {} \
                 ({bytes} bytes, {} sectors).",
                bin_path.display(),
                bytes / format.sector_size() as u64
            );
            println!(
                "Extracting a mountable filesystem from Mode 2 is consumer territory — \
                 see docs/consuming-cd-da-reader.md (\"Mode 2\")."
            );
        }
        other => {
            return Err(format!(
                "data track #{} detected as {other:?}, which is unexpected for a data track",
                data_track.number
            )
            .into());
        }
    }

    Ok(())
}

/// Stream one track straight to a file in `format`, without ever holding the
/// whole track in memory. Returns the number of bytes written.
///
/// Uses the streaming API so peak memory is one chunk (~64 KB) instead of the
/// entire track, which matters for large data images.
fn stream_track_to_file(
    reader: &CdReader,
    toc: &Toc,
    track_no: u8,
    format: SectorReadFormat,
    path: &Path,
) -> Result<u64, Box<dyn std::error::Error>> {
    let options = TrackStreamOptions::default().with_format(format);
    let mut stream = reader.open_track_stream_with_options(toc, track_no, options)?;

    let total_sectors = stream.total_sectors();
    let mut writer = BufWriter::new(File::create(path)?);
    let mut written = 0u64;

    while let Some(chunk) = stream.next_chunk()? {
        writer.write_all(&chunk)?;
        written += chunk.len() as u64;

        let done = stream.current_sector();
        let pct = done as f32 / total_sectors as f32 * 100.0;
        eprint!("\r  {done}/{total_sectors} sectors ({pct:5.1}%)");
    }
    eprintln!("\r  {total_sectors}/{total_sectors} sectors (100.0%)");

    writer.flush()?;
    Ok(written)
}

fn print_mount_hint(path: &str) {
    println!("Mount it and explore the files:");
    if cfg!(target_os = "macos") {
        println!("  hdiutil attach \"{path}\"");
    } else if cfg!(target_os = "linux") {
        println!("  sudo mount -o loop,ro \"{path}\" /mnt/cd");
    } else if cfg!(target_os = "windows") {
        println!("  PowerShell: Mount-DiskImage -ImagePath \"{path}\"");
    }
}
