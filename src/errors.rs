use std::fmt;

#[derive(Debug, Clone, Copy)]
pub enum ScsiOp {
    ReadToc,
    ReadCd,
    ReadSubChannel,
}

#[derive(Debug, Clone)]
pub struct ScsiError {
    pub op: ScsiOp,
    pub lba: Option<u32>,
    pub sectors: Option<u32>,
    pub scsi_status: u8,
    pub sense_key: Option<u8>,
    pub asc: Option<u8>,
    pub ascq: Option<u8>,
}

#[derive(Debug)]
pub enum CdReaderError {
    Io(std::io::Error),
    Scsi(ScsiError),
    Parse(String),
}

impl fmt::Display for CdReaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Scsi(err) => write!(
                f,
                "SCSI {:?} failed (status=0x{:02x}, lba={:?}, sectors={:?}, sense_key={:?}, asc={:?}, ascq={:?})",
                err.op, err.scsi_status, err.lba, err.sectors, err.sense_key, err.asc, err.ascq
            ),
            Self::Parse(msg) => write!(f, "parse error: {msg}"),
        }
    }
}

impl std::error::Error for CdReaderError {}

impl From<std::io::Error> for CdReaderError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
