use std::fmt;

/// SCSI command groups issued by this library.
#[derive(Debug, Clone, Copy)]
pub enum ScsiOp {
    /// `READ TOC/PMA/ATIP` command (opcode `0x43`) for TOC/session metadata.
    ReadToc,
    /// `READ CD` command (opcode `0xBE`) for CD-DA sector payload (2352 bytes/sector).
    ReadCd,
    /// `READ SUB-CHANNEL` command for Q-channel/subcode metadata.
    ReadSubChannel,
}

/// Structured SCSI failure context captured at the call site.
///
/// This keeps transport/protocol details (status + sense) separate from plain I/O failures,
/// which allows retry logic and application diagnostics to branch on SCSI metadata.
#[derive(Debug, Clone)]
pub struct ScsiError {
    /// Operation that failed.
    pub op: ScsiOp,
    /// Starting logical block address used by the failed command, when applicable.
    pub lba: Option<u32>,
    /// Sector count requested by the failed command, when applicable.
    pub sectors: Option<u32>,
    /// SCSI status byte reported by the device (for example `0x02` for CHECK CONDITION).
    pub scsi_status: u8,
    /// Sense key nibble from fixed-format sense data (if sense data was returned).
    pub sense_key: Option<u8>,
    /// Additional Sense Code from sense data (if available).
    pub asc: Option<u8>,
    /// Additional Sense Code Qualifier paired with `asc` (if available).
    pub ascq: Option<u8>,
}

/// Top-level error type returned by `cd-da-reader`.
#[derive(Debug)]
pub enum CdReaderError {
    /// OS/transport I/O error (open/ioctl/DeviceIoControl/FFI command failure, etc.).
    Io(std::io::Error),
    /// Device reported a SCSI command failure with status/sense context.
    Scsi(ScsiError),
    /// Parsing failure for command payloads (TOC/CD-TEXT/subchannel parsing).
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
