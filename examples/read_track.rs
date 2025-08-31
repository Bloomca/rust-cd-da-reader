use cd_da_reader::CdReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    read_cd()?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn read_cd() -> Result<(), Box<dyn std::error::Error>> {
    let reader = CdReader::open(r"\\.\E:")?;
    let toc = reader.read_toc()?;
    println!("{:#?}", toc);

    let data = reader.read_track(&toc, 11)?;
    let wav_track = CdReader::create_wav(data);
    std::fs::write("myfile.wav", wav_track)?;

    Ok(())
}

#[cfg(target_os = "macos")]
fn read_cd() -> Result<(), Box<dyn std::error::Error>> {
    let reader = CdReader::open("disk6")?;
    let toc = reader.read_toc()?;
    println!("{:#?}", toc);

    let data = reader.read_track(&toc, 11)?;
    let wav_track = CdReader::create_wav(data);
    std::fs::write("myfile.wav", wav_track)?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn read_cd() -> Result<(), Box<dyn std::error::Error>> {
    let reader = CdReader::open("/dev/sr0")?;
    let toc = reader.read_toc()?;
    println!("{:#?}", toc);

    let data = reader.read_track(&toc, 11)?;
    let wav_track = CdReader::create_wav(data);
    std::fs::write("myfile.wav", wav_track)?;

    Ok(())
}
