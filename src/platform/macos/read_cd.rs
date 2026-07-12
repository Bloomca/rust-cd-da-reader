use std::{ptr, slice};

use super::device::Drive;
use super::ffi::{MacScsiError, cd_free, map_error, read_cd_sectors};
use crate::{CdReaderError, ScsiOp, SectorReadMode};

pub(super) fn read_cd_chunk(
    drive: &Drive,
    lba: u32,
    sectors: u32,
    mode: SectorReadMode,
) -> Result<Vec<u8>, CdReaderError> {
    let mut buffer: *mut u8 = ptr::null_mut();
    let mut len = 0u32;
    let mut error = MacScsiError::default();

    let success = unsafe {
        read_cd_sectors(
            drive.fd(),
            lba,
            sectors,
            mode_id(mode),
            &mut buffer,
            &mut len,
            &mut error,
        )
    };
    if !success {
        return Err(map_error(error, ScsiOp::ReadCd, Some(lba), Some(sectors)));
    }

    let data = unsafe { slice::from_raw_parts(buffer, len as usize) }.to_vec();
    unsafe { cd_free(buffer.cast()) };

    Ok(data)
}

// Discriminant understood by the native `read_cd_sectors` implementation.
fn mode_id(mode: SectorReadMode) -> u32 {
    match mode {
        SectorReadMode::Audio => 0,
        SectorReadMode::DataCooked => 1,
        SectorReadMode::DataRaw => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::mode_id;
    use crate::SectorReadMode;

    #[test]
    fn maps_sector_modes_to_native_ids() {
        assert_eq!(mode_id(SectorReadMode::Audio), 0);
        assert_eq!(mode_id(SectorReadMode::DataCooked), 1);
        assert_eq!(mode_id(SectorReadMode::DataRaw), 2);
    }
}
