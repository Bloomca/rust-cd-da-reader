use super::device::Drive;
use super::spti::{CommandContext, execute_read};
use crate::data_reader::track_information::{
    TRACK_INFORMATION_RESPONSE_SIZE, TrackInformation, build_read_track_information_cdb,
    parse_track_information,
};
use crate::{CdReaderError, ScsiOp};

const TRACK_INFORMATION_TIMEOUT_SECONDS: u32 = 10;

pub(super) fn read_track_information(
    drive: &Drive,
    track_number: u8,
) -> Result<TrackInformation, CdReaderError> {
    let mut data = vec![0u8; TRACK_INFORMATION_RESPONSE_SIZE];
    let cdb =
        build_read_track_information_cdb(track_number, TRACK_INFORMATION_RESPONSE_SIZE as u16);
    let transferred = execute_read(
        drive.handle(),
        &cdb,
        &mut data,
        TRACK_INFORMATION_TIMEOUT_SECONDS,
        CommandContext {
            op: ScsiOp::ReadTrackInformation,
            lba: None,
            sectors: None,
        },
    )?;
    data.truncate(transferred);

    parse_track_information(&data).map_err(|error| CdReaderError::Parse(error.to_string()))
}
