use cd_da_reader::CdDevice;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    read_cd()?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn read_cd() -> Result<(), Box<dyn std::error::Error>> {
    let reader = CdDevice::open(r"\\.\E:")?;
    let toc = reader.read_toc()?;
    println!("{:#?}", toc);

    let data = reader.read_track(toc, 6)?;

    let mut header = create_wav_header(data.len() as u32);
    header.extend_from_slice(&data);
    std::fs::write("myfile.wav", header)?;

    Ok(())
}

#[cfg(target_os = "macos")]
fn read_cd() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn read_cd() {
    println!("CD reading not implemented for Linux yet");
    Ok(())
}

// just to test for now, it probably won't be exported
fn create_wav_header(pcm_data_size: u32) -> Vec<u8> {
    let mut header = Vec::with_capacity(44);

    // RIFF header
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(pcm_data_size + 36).to_le_bytes()); // file size - 8
    header.extend_from_slice(b"WAVE");

    // fmt chunk
    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    header.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    header.extend_from_slice(&2u16.to_le_bytes()); // channels
    header.extend_from_slice(&44100u32.to_le_bytes()); // sample rate
    header.extend_from_slice(&176400u32.to_le_bytes()); // byte rate
    header.extend_from_slice(&4u16.to_le_bytes()); // block align
    header.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    // data chunk header
    header.extend_from_slice(b"data");
    header.extend_from_slice(&pcm_data_size.to_le_bytes());

    header
}
