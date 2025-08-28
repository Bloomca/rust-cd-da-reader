use std::{ffi::CString, ptr, slice};

use crate::Toc;
use crate::parse_toc::parse_toc;

#[link(name = "macos_cd_shim", kind = "static")]
unsafe extern "C" {
    fn start_da_guard(bsd_name: *const libc::c_char);
    fn stop_da_guard();
    fn cd_read_toc(bsd_name: *const libc::c_char, out_buf: *mut *mut u8, out_len: *mut u32)
    -> bool;
    fn read_cd_audio(
        bsd_name: *const libc::c_char,
        lba: u32,
        sectors: u32,
        out_buf: *mut *mut u8,
        out_len: *mut u32,
    ) -> bool;
    fn cd_free(p: *mut libc::c_void);
}

pub fn mac_read_toc(path: &str) -> std::io::Result<Toc> {
    let bsd = CString::new(path).unwrap();
    let mut buf: *mut u8 = ptr::null_mut();
    let mut len: u32 = 0;

    let ok = unsafe { cd_read_toc(bsd.as_ptr(), &mut buf, &mut len) };
    if !ok {
        return Err(std::io::Error::other("TOC read failed"));
    }
    let data = unsafe { slice::from_raw_parts(buf, len as usize) };
    println!(
        "TOC len={}, first 16 bytes: {:02X?}",
        len,
        &data[..16.min(data.len())]
    );

    // `.to_vec()` will copy the data, so we can free it safely after
    let result = parse_toc(data.to_vec());

    unsafe { cd_free(buf as *mut _) };

    result
}

pub fn mac_start_da_guard(path: &str) {
    let bsd = CString::new(path).unwrap();
    unsafe { start_da_guard(bsd.as_ptr()) };
}

pub fn mac_stop_da_guard() {
    unsafe { stop_da_guard() };
}

pub fn mac_read_track(path: &str, toc: &Toc, track_no: u8) -> std::io::Result<Vec<u8>> {
    let bsd = CString::new(path).unwrap();
    let mut buf: *mut u8 = ptr::null_mut();
    let mut len: u32 = 0;

    let idx = toc
        .tracks
        .iter()
        .position(|t| t.number == track_no)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "track not in TOC"))?;

    let start_lba = toc.tracks[idx].start_lba as u32;

    // Determine end LBA (next track start, or lead-out for the last track)
    let end_lba: u32 = if (idx + 1) < toc.tracks.len() {
        toc.tracks[idx + 1].start_lba as u32
    } else {
        // read_leadout_lba(handle)?
        return Err(std::io::Error::other(
            "Last track is not supported right now",
        ));
    };

    if end_lba <= start_lba {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "bad TOC bounds",
        ));
    }

    let sectors = end_lba - start_lba;
    let ok = unsafe { read_cd_audio(bsd.as_ptr(), start_lba, sectors, &mut buf, &mut len) };

    if !ok {
        return Err(std::io::Error::other("TOC read failed"));
    }

    let data = unsafe { slice::from_raw_parts(buf, len as usize) };
    println!(
        "TOC len={}, first 16 bytes: {:02X?}",
        len,
        &data[..16.min(data.len())]
    );

    // `.to_vec()` will copy the data, so we can free it safely after
    let result = data.to_vec();

    unsafe { cd_free(buf as *mut _) };

    Ok(result)
}
