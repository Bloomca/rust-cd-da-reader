use super::device::Drive;
use super::spti::{CommandContext, execute_read};
use crate::parse_toc::parse_toc;
use crate::{CdReaderError, ScsiOp, Toc};

const TOC_BUFFER_SIZE: usize = 2048;
const TOC_TIMEOUT_SECONDS: u32 = 10;

pub(super) fn read_toc(drive: &Drive) -> Result<Toc, CdReaderError> {
    let mut data = vec![0u8; TOC_BUFFER_SIZE];
    let cdb = build_read_toc_cdb(TOC_BUFFER_SIZE);
    let transferred = execute_read(
        drive.handle(),
        &cdb,
        &mut data,
        TOC_TIMEOUT_SECONDS,
        CommandContext {
            op: ScsiOp::ReadToc,
            lba: None,
            sectors: None,
        },
    )?;
    data.truncate(transferred);

    parse_toc(data).map_err(|error| CdReaderError::Parse(error.to_string()))
}

fn build_read_toc_cdb(allocation_len: usize) -> [u8; 10] {
    let mut cdb = [0u8; 10];
    cdb[0] = 0x43; // READ TOC/PMA/ATIP
    cdb[1] = 0x00; // LBA format
    cdb[2] = 0x00; // TOC format
    cdb[6] = 0x00; // Start with the first track/session
    cdb[7] = ((allocation_len >> 8) & 0xFF) as u8;
    cdb[8] = (allocation_len & 0xFF) as u8;
    cdb
}

#[cfg(test)]
mod tests {
    use super::build_read_toc_cdb;

    #[test]
    fn builds_read_toc_cdb() {
        assert_eq!(
            build_read_toc_cdb(2048),
            [0x43, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00]
        );
    }
}
