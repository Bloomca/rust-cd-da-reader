use std::{ffi::CString, ptr, slice};

use crate::Toc;
use crate::parse_toc::parse_toc;

#[link(name = "macos_cd_shim", kind = "static")]
unsafe extern "C" {
    fn start_da_guard(bsd_name: *const libc::c_char);
    fn stop_da_guard();
    fn cd_read_toc(bsd_name: *const libc::c_char, out_buf: *mut *mut u8, out_len: *mut u32)
    -> bool;
    fn cd_free(p: *mut libc::c_void);
}

pub fn mac_read_toc(path: &str) -> std::io::Result<Toc> {
    let bsd = CString::new(path).unwrap();
    let mut buf: *mut u8 = ptr::null_mut();
    let mut len: u32 = 0;

    let ok = unsafe { cd_read_toc(bsd.as_ptr(), &mut buf, &mut len) };
    if !ok {
        eprintln!("TOC read failed");
        std::process::exit(1);
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
