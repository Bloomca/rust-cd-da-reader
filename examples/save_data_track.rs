//! Save the data track of a mixed-mode / enhanced ("CD-Extra") disc as a
//! mountable image, then print how to mount it.
//!
//! This is the end-to-end data-track workflow:
//!   1. read the TOC and pick the (first) data track,
//!   2. auto-detect its sector format with `detect_track_format`,
//!   3. for Mode 1, read it **cooked** (2048 B/sector) — that is exactly the
//!      ISO 9660 filesystem image, so it writes straight to a `.iso` you can
//!      mount and explore,
//!   4. Mode 2 is detected but not auto-cooked here (see the note it prints and
//!      `docs/consuming-cd-da-reader.md`).
//!
//! Run with: `cargo run --example save_data_track`
mod common;

use cd_da_reader::{CdReader, ReadOptions, SectorReadFormat};

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
            let options = ReadOptions::default().with_format(SectorReadFormat::Mode1Cooked);
            let image = reader.read_track_with_options(&toc, data_track.number, &options)?;

            let iso_path = output_dir.join(format!("track{:02}.iso", data_track.number));
            std::fs::write(&iso_path, &image)?;
            println!(
                "Wrote {} ({} bytes, {} sectors)\n",
                iso_path.display(),
                image.len(),
                image.len() / 2048
            );
            print_mount_hint(&iso_path.display().to_string());
        }
        SectorReadFormat::Mode2Raw => {
            // Mode 2 forms are a per-sector property; producing a clean cooked
            // payload requires inspecting each sector's XA subheader, which is
            // left to the consumer. We save the complete raw sectors so nothing
            // is lost, and point at the docs.
            let options = ReadOptions::default().with_format(SectorReadFormat::Mode2Raw);
            let raw = reader.read_track_with_options(&toc, data_track.number, &options)?;

            let bin_path = output_dir.join(format!("track{:02}.mode2.bin", data_track.number));
            std::fs::write(&bin_path, &raw)?;
            println!(
                "This is a Mode 2 track. Saved complete raw sectors to {} \
                 ({} bytes, {} sectors).",
                bin_path.display(),
                raw.len(),
                raw.len() / 2352
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
