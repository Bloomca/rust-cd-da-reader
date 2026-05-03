/// Lists all optical drives detected on the system and whether they contain an audio CD.
use cd_da_reader::CdReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let drives = CdReader::list_drives()?;

    if drives.is_empty() {
        println!("No optical drives found.");
        return Ok(());
    }

    println!("Found {} drive(s):\n", drives.len());
    for drive in &drives {
        let name = drive.display_name.as_deref().unwrap_or("(unknown)");
        let status = if drive.has_audio_cd {
            "audio CD inserted"
        } else {
            "no audio CD"
        };
        println!(
            "Drive: {}, name: {}, status: [{}]",
            drive.path, name, status
        );
    }

    Ok(())
}
