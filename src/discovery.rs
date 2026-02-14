use crate::{CdReader, CdReaderError};

#[derive(Debug, Clone)]
pub struct DriveInfo {
    pub path: String,
    pub display_name: Option<String>,
    pub has_audio_cd: bool,
}

impl CdReader {
    /// Enumerate candidate optical drives and probe whether they currently have an audio CD.
    pub fn list_drives() -> Result<Vec<DriveInfo>, CdReaderError> {
        #[cfg(target_os = "windows")]
        let mut paths = crate::windows::list_drive_paths().map_err(CdReaderError::Io)?;

        #[cfg(target_os = "macos")]
        let mut paths = crate::macos::list_drive_paths().map_err(CdReaderError::Io)?;

        #[cfg(target_os = "linux")]
        let mut paths = crate::linux::list_drive_paths().map_err(CdReaderError::Io)?;

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        {
            compile_error!("Unsupported platform")
        }

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
        let chosen = pick_default_drive_path(&drives).ok_or_else(|| {
            CdReaderError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no usable audio CD drive found",
            ))
        })?;

        Self::open(chosen).map_err(CdReaderError::Io)
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
