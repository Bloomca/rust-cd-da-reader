use cd_da_reader::CdDevice;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let reader = CdDevice::open(r"\\.\E:")?;
    let toc = reader.read_toc()?;
    println!("{:#?}", toc);
    Ok(())
}