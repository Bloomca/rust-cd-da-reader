use std::{ptr, slice};

use super::device::Drive;
use super::ffi::{MacScsiError, cd_free, cd_read_track_information, map_error};
use crate::data_reader::track_information::{TrackInformation, parse_track_information};
use crate::{CdReaderError, ScsiOp};

pub(super) fn read_track_information(
    drive: &Drive,
    track_number: u8,
) -> Result<TrackInformation, CdReaderError> {
    let mut buffer: *mut u8 = ptr::null_mut();
    let mut len = 0u32;
    let mut error = MacScsiError::default();

    let success = unsafe {
        cd_read_track_information(drive.fd(), track_number, &mut buffer, &mut len, &mut error)
    };
    if !success {
        return Err(map_error(error, ScsiOp::ReadTrackInformation, None, None));
    }

    let data = unsafe { slice::from_raw_parts(buffer, len as usize) }.to_vec();
    unsafe { cd_free(buffer.cast()) };

    parse_track_information(&data).map_err(|error| CdReaderError::Parse(error.to_string()))
}
