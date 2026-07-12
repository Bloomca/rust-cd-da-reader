#![cfg_attr(not(test), allow(dead_code))]

use std::fmt;

const RAW_SECTOR_SIZE: usize = 2352;
const MODE_OFFSET: usize = 15;
const XA_SUBHEADER_FIRST: std::ops::Range<usize> = 16..20;
const XA_SUBHEADER_SECOND: std::ops::Range<usize> = 20..24;
const XA_SUBMODE_OFFSET: usize = 18;
const XA_FORM2_BIT: u8 = 0x20;
const XA_CONTENT_TYPE_MASK: u8 = 0x0E;
const SYNC_PATTERN: [u8; 12] = [
    0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RawSectorMode {
    Mode1,
    Mode2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum XaForm {
    Form1,
    Form2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RawSectorInfo {
    pub(super) mode: RawSectorMode,
    /// XA form observed in this sector when both subheader copies agree.
    pub(super) xa_form: Option<XaForm>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RawSectorParseError {
    InvalidLength { actual: usize },
    InvalidSync,
    UnsupportedMode(u8),
}

impl fmt::Display for RawSectorParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength { actual } => {
                write!(
                    f,
                    "raw sector has {actual} bytes, expected {RAW_SECTOR_SIZE}"
                )
            }
            Self::InvalidSync => write!(f, "raw sector has an invalid sync pattern"),
            Self::UnsupportedMode(mode) => {
                write!(f, "raw sector has unsupported mode 0x{mode:02x}")
            }
        }
    }
}

impl std::error::Error for RawSectorParseError {}

/// Parse one complete 2352-byte data sector.
pub(super) fn parse_raw_sector(sector: &[u8]) -> Result<RawSectorInfo, RawSectorParseError> {
    if sector.len() != RAW_SECTOR_SIZE {
        return Err(RawSectorParseError::InvalidLength {
            actual: sector.len(),
        });
    }

    if sector[..SYNC_PATTERN.len()] != SYNC_PATTERN {
        return Err(RawSectorParseError::InvalidSync);
    }

    match sector[MODE_OFFSET] {
        0x01 => Ok(RawSectorInfo {
            mode: RawSectorMode::Mode1,
            xa_form: None,
        }),
        0x02 => Ok(RawSectorInfo {
            mode: RawSectorMode::Mode2,
            xa_form: parse_xa_form(sector),
        }),
        mode => Err(RawSectorParseError::UnsupportedMode(mode)),
    }
}

fn parse_xa_form(sector: &[u8]) -> Option<XaForm> {
    if sector[XA_SUBHEADER_FIRST] != sector[XA_SUBHEADER_SECOND] {
        return None;
    }

    let submode = sector[XA_SUBMODE_OFFSET];
    if submode & XA_CONTENT_TYPE_MASK == 0 {
        return None;
    }

    if submode & XA_FORM2_BIT == 0 {
        Some(XaForm::Form1)
    } else {
        Some(XaForm::Form2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sector(mode: u8) -> [u8; RAW_SECTOR_SIZE] {
        let mut sector = [0u8; RAW_SECTOR_SIZE];
        sector[..SYNC_PATTERN.len()].copy_from_slice(&SYNC_PATTERN);
        sector[MODE_OFFSET] = mode;
        sector
    }

    fn set_xa_subheader(sector: &mut [u8], submode: u8) {
        let subheader = [1, 2, submode, 4];
        sector[XA_SUBHEADER_FIRST].copy_from_slice(&subheader);
        sector[XA_SUBHEADER_SECOND].copy_from_slice(&subheader);
    }

    #[test]
    fn parses_mode1() {
        assert_eq!(
            parse_raw_sector(&sector(1)).unwrap(),
            RawSectorInfo {
                mode: RawSectorMode::Mode1,
                xa_form: None,
            }
        );
    }

    #[test]
    fn parses_mode2_form1() {
        let mut sector = sector(2);
        set_xa_subheader(&mut sector, 0x08);

        assert_eq!(
            parse_raw_sector(&sector).unwrap(),
            RawSectorInfo {
                mode: RawSectorMode::Mode2,
                xa_form: Some(XaForm::Form1),
            }
        );
    }

    #[test]
    fn parses_mode2_form2() {
        let mut sector = sector(2);
        set_xa_subheader(&mut sector, 0x28);

        assert_eq!(
            parse_raw_sector(&sector).unwrap(),
            RawSectorInfo {
                mode: RawSectorMode::Mode2,
                xa_form: Some(XaForm::Form2),
            }
        );
    }

    #[test]
    fn leaves_xa_form_unknown_without_a_valid_xa_content_type() {
        assert_eq!(
            parse_raw_sector(&sector(2)).unwrap(),
            RawSectorInfo {
                mode: RawSectorMode::Mode2,
                xa_form: None,
            }
        );
    }

    #[test]
    fn leaves_xa_form_unknown_when_subheaders_disagree() {
        let mut sector = sector(2);
        sector[XA_SUBHEADER_FIRST].copy_from_slice(&[1, 2, 0x08, 4]);
        sector[XA_SUBHEADER_SECOND].copy_from_slice(&[1, 2, 0x28, 4]);

        assert_eq!(
            parse_raw_sector(&sector).unwrap(),
            RawSectorInfo {
                mode: RawSectorMode::Mode2,
                xa_form: None,
            }
        );
    }

    #[test]
    fn rejects_invalid_length() {
        assert_eq!(
            parse_raw_sector(&[0; RAW_SECTOR_SIZE - 1]),
            Err(RawSectorParseError::InvalidLength {
                actual: RAW_SECTOR_SIZE - 1,
            })
        );
    }

    #[test]
    fn rejects_invalid_sync() {
        assert_eq!(
            parse_raw_sector(&[0; RAW_SECTOR_SIZE]),
            Err(RawSectorParseError::InvalidSync)
        );
    }

    #[test]
    fn rejects_unsupported_mode() {
        assert_eq!(
            parse_raw_sector(&sector(0)),
            Err(RawSectorParseError::UnsupportedMode(0))
        );
        assert_eq!(
            parse_raw_sector(&sector(3)),
            Err(RawSectorParseError::UnsupportedMode(3))
        );
    }
}
