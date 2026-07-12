use std::{ptr, slice};

use super::device::Drive;
use super::ffi::{MacScsiError, cd_free, cd_read_toc, map_error};
use crate::parse_toc::parse_toc;
use crate::{CdReaderError, ScsiOp, Toc};

pub(super) fn read_toc(drive: &Drive) -> Result<Toc, CdReaderError> {
    let mut buffer: *mut u8 = ptr::null_mut();
    let mut len = 0u32;
    let mut error = MacScsiError::default();

    let success = unsafe { cd_read_toc(drive.fd(), &mut buffer, &mut len, &mut error) };
    if !success {
        return Err(map_error(error, ScsiOp::ReadToc, None, None));
    }

    let data = unsafe { slice::from_raw_parts(buffer, len as usize) }.to_vec();
    unsafe { cd_free(buffer.cast()) };

    parse_toc(data).map_err(|error| CdReaderError::Parse(error.to_string()))
}
