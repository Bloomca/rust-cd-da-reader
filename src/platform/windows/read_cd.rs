use super::spti::{CommandContext, execute_read};
use crate::data_reader::{SectorReadMode, build_read_cd_cdb};
use crate::{CdReaderError, ScsiOp};

const READ_CD_TIMEOUT_SECONDS: u32 = 30;

pub(crate) fn read_cd_chunk(
    lba: u32,
    sectors: u32,
    mode: SectorReadMode,
) -> Result<Vec<u8>, CdReaderError> {
    let mut chunk = vec![0u8; sectors as usize * mode.sector_size()];
    let cdb = build_read_cd_cdb(lba, sectors, mode);
    let transferred = execute_read(
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
