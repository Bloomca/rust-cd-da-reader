/// Detects the default read format for every track on the inserted disc.
use cd_da_reader::CdReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let reader = CdReader::open_default()?;
    let toc = reader.read_toc()?;

    for track in &toc.tracks {
        let format = reader.detect_track_format(track)?;
        println!("Track #{:>2}: {format:?}", track.number);
    }

    Ok(())
}
