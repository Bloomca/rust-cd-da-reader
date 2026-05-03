use crate::{CdReader, CdReaderError};

/// Information about all found drives. This info is not tested extensively, and in
/// general it is encouraged to provide a disk drive directly.
#[derive(Debug, Clone)]
pub struct DriveInfo {
    /// Path to the drive, which can be something like 'disk6' on macOS,
    /// '\\.\E:' on Windows, and '/dev/sr0' on Linux
    pub path: String,
    /// Just the device name, without the full path for the OS
    pub display_name: Option<String>,
    /// Whether the current disc appears to contain at least one audio track.
    pub has_audio_cd: bool,
}

impl CdReader {
    /// Enumerate candidate optical drives and probe whether they currently have an audio CD.
    ///
    /// On macOS, this uses the `IOCDMedia` objects published in the I/O Registry and
    /// inspects their TOC property without claiming exclusive access.
    #[cfg(target_os = "macos")]
    pub fn list_drives() -> Result<Vec<DriveInfo>, CdReaderError> {
        crate::macos::list_drives().map_err(CdReaderError::Io)
    }

    /// Enumerate candidate optical drives and probe whether they currently have an audio CD.
    ///
    /// On Windows, we try to read type of every drive from A to Z. On Linux, we read
    /// /sys/class/block directory and check every entry starting with "sr"
    #[cfg(not(target_os = "macos"))]
    pub fn list_drives() -> Result<Vec<DriveInfo>, CdReaderError> {
        let mut paths = {
            #[cfg(target_os = "windows")]
            {
                crate::windows::list_drive_paths().map_err(CdReaderError::Io)?
            }

            #[cfg(target_os = "linux")]
            {
                crate::linux::list_drive_paths().map_err(CdReaderError::Io)?
            }

            #[cfg(not(any(target_os = "windows", target_os = "linux")))]
            {
                compile_error!("Unsupported platform")
            }
        };

        paths.sort();
        paths.dedup();

        let mut drives = Vec::with_capacity(paths.len());
        for path in paths {
            let has_audio_cd = match Self::open(&path) {
                Ok(reader) => match reader.read_toc() {
                    Ok(toc) => toc.tracks.iter().any(|track| track.is_audio),
                    Err(_) => false,
                },
                Err(_) => false,
            };

            drives.push(DriveInfo {
                display_name: Some(path.clone()),
                path,
                has_audio_cd,
            });
        }

        Ok(drives)
    }

    /// Open the first discovered drive that currently has an audio CD.
    ///
    /// On macOS, we use the passive `IOCDMedia` discovery path and then open
    /// the matching BSD device name without claiming exclusive access.
    #[cfg(target_os = "macos")]
    pub fn open_default() -> Result<Self, CdReaderError> {
        let drives = Self::list_drives()?;
        let chosen = drives
            .iter()
            .find(|drive| drive.has_audio_cd)
            .map(|drive| drive.path.as_str())
            .ok_or_else(|| {
                CdReaderError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no usable audio CD drive found",
                ))
            })?;

        Self::open(chosen).map_err(CdReaderError::Io)
    }

    /// Open the first discovered drive that currently has an audio CD.
    ///
    /// On Windows and Linux, we get the first device from the list and
    /// try to open it, returning an error if it fails.
    #[cfg(not(target_os = "macos"))]
    pub fn open_default() -> Result<Self, CdReaderError> {
        let drives = Self::list_drives()?;
        let chosen = pick_default_drive_path(&drives).ok_or_else(|| {
            CdReaderError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no usable audio CD drive found",
            ))
        })?;

        Self::open(chosen).map_err(CdReaderError::Io)
    }
}

#[cfg(any(test, not(target_os = "macos")))]
fn pick_default_drive_path(drives: &[DriveInfo]) -> Option<&str> {
    drives
        .iter()
        .find(|drive| drive.has_audio_cd)
        .map(|drive| drive.path.as_str())
}

#[cfg(test)]
mod tests {
    use super::{DriveInfo, pick_default_drive_path};

    #[test]
    fn chooses_first_audio_drive() {
        let drives = vec![
            DriveInfo {
                path: "disk10".to_string(),
                display_name: None,
                has_audio_cd: false,
            },
            DriveInfo {
                path: "disk11".to_string(),
                display_name: None,
                has_audio_cd: true,
            },
            DriveInfo {
                path: "disk12".to_string(),
                display_name: None,
                has_audio_cd: true,
            },
        ];

        assert_eq!(pick_default_drive_path(&drives), Some("disk11"));
    }

    #[test]
    fn returns_none_when_no_audio_drive() {
        let drives = vec![DriveInfo {
            path: "/dev/sr0".to_string(),
            display_name: None,
            has_audio_cd: false,
        }];

        assert_eq!(pick_default_drive_path(&drives), None);
    }
}
