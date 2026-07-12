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
    pub fn list_drives() -> Result<Vec<DriveInfo>, CdReaderError> {
        let mut paths = crate::platform::list_drive_paths()?;
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
    pub fn open_default() -> Result<Self, CdReaderError> {
        let drives = Self::list_drives()?;
        let chosen = pick_default_drive_path(&drives).ok_or(CdReaderError::NoUsableDrive)?;

        Self::open(chosen)
    }
}

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
