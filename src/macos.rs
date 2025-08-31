use std::{ffi::CString, ptr, slice};

use crate::Toc;
use crate::parse_toc::parse_toc;

#[link(name = "macos_cd_shim", kind = "static")]
unsafe extern "C" {
    fn start_da_guard(bsd_name: *const libc::c_char);
    fn stop_da_guard();
    fn cd_read_toc(out_buf: *mut *mut u8, out_len: *mut u32) -> bool;
    fn read_cd_audio(lba: u32, sectors: u32, out_buf: *mut *mut u8, out_len: *mut u32) -> bool;
    fn cd_free(p: *mut libc::c_void);
    fn get_dev_svc(bsd_name: *const libc::c_char) -> bool;
    fn reset_dev_scv();
}

pub fn open_drive(path: &str) -> std::io::Result<()> {
    let bsd = CString::new(path).unwrap();
    unsafe { start_da_guard(bsd.as_ptr()) };
    let result = unsafe { get_dev_svc(bsd.as_ptr()) };

    if !result {
        return Err(std::io::Error::other("could not get device"));
    }

    Ok(())
}

pub fn close_drive() {
    unsafe { reset_dev_scv() };
    unsafe { stop_da_guard() };
}

pub fn read_toc() -> std::io::Result<Toc> {
    let mut buf: *mut u8 = ptr::null_mut();
    let mut len: u32 = 0;

    let ok = unsafe { cd_read_toc(&mut buf, &mut len) };
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

pub fn read_track(toc: &Toc, track_no: u8) -> std::io::Result<Vec<u8>> {
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
    let ok = unsafe { read_cd_audio(start_lba, sectors, &mut buf, &mut len) };

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
