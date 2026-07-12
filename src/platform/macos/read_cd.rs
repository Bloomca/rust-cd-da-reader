use std::{ptr, slice};

use super::device::Drive;
use super::ffi::{MacScsiError, cd_free, map_error, read_cd_sectors};
use crate::{CdReaderError, ScsiOp, SectorReadFormat};

pub(super) fn read_cd_chunk(
    drive: &Drive,
    lba: u32,
    sectors: u32,
    format: SectorReadFormat,
) -> Result<Vec<u8>, CdReaderError> {
    let mut buffer: *mut u8 = ptr::null_mut();
    let mut len = 0u32;
    let mut error = MacScsiError::default();

    let success = unsafe {
        read_cd_sectors(
            drive.fd(),
            lba,
            sectors,
            format_id(format),
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
fn format_id(format: SectorReadFormat) -> u32 {
    match format {
        SectorReadFormat::Audio => 0,
        SectorReadFormat::Mode1Cooked => 1,
        SectorReadFormat::Mode1Raw => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::format_id;
    use crate::SectorReadFormat;

    #[test]
    fn maps_sector_formats_to_native_ids() {
        assert_eq!(format_id(SectorReadFormat::Audio), 0);
        assert_eq!(format_id(SectorReadFormat::Mode1Cooked), 1);
        assert_eq!(format_id(SectorReadFormat::Mode1Raw), 2);
    }
}
