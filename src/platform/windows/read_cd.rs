use super::device::Drive;
use super::spti::{CommandContext, execute_read};
use crate::data_reader::{SectorReadFormat, build_read_cd_cdb};
use crate::{CdReaderError, ScsiOp};

const READ_CD_TIMEOUT_SECONDS: u32 = 30;

pub(super) fn read_cd_chunk(
    drive: &Drive,
    lba: u32,
    sectors: u32,
    format: SectorReadFormat,
) -> Result<Vec<u8>, CdReaderError> {
    let mut chunk = vec![0u8; sectors as usize * format.sector_size()];
    let cdb = build_read_cd_cdb(lba, sectors, format);
    let transferred = execute_read(
        drive.handle(),
        &cdb,
        &mut chunk,
        READ_CD_TIMEOUT_SECONDS,
        CommandContext {
            op: ScsiOp::ReadCd,
            lba: Some(lba),
            sectors: Some(sectors),
        },
    )?;
    chunk.truncate(transferred);

    Ok(chunk)
}
