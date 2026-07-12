mod device;
mod read_cd;
mod sg_io;
mod toc;

pub(crate) use device::{Drive, list_drive_paths};

use crate::{CdReaderError, SectorReadFormat, Toc};

impl Drive {
    pub(crate) fn read_toc(&self) -> Result<Toc, CdReaderError> {
        toc::read_toc(self)
    }

    pub(crate) fn read_cd_chunk(
        &self,
        lba: u32,
        sectors: u32,
        format: SectorReadFormat,
    ) -> Result<Vec<u8>, CdReaderError> {
        read_cd::read_cd_chunk(self, lba, sectors, format)
    }
}
