//! User-space memory access utilities.
//!
//! Page-boundary-aware copy routines for transferring data between
//! kernel buffers and user-space virtual addresses.

use alloc::string::String;

use crate::memory;
use crate::page_mapper::PageMapper;

/// Copy `data` from a kernel buffer into user-space at `user_addr`.
/// Returns `Ok(())` on success or `Err(())` if address translation fails.
pub fn copy_out(page_map: &PageMapper, user_addr: usize, data: &[u8]) -> Result<(), ()> {
    let mut remaining = data;
    let mut user_ptr = user_addr;

    while !remaining.is_empty() {
        let page_offset = user_ptr & (memory::PAGE_SIZE - 1);
        let chunk_size = remaining.len().min(memory::PAGE_SIZE - page_offset);

        let phys_addr = page_map.virt_to_phys(user_ptr).ok_or(())?;

        unsafe {
            core::ptr::copy_nonoverlapping(
                remaining.as_ptr(),
                phys_addr as *mut u8,
                chunk_size,
            );
        }

        user_ptr += chunk_size;
        remaining = &remaining[chunk_size..];
    }

    Ok(())
}

/// Copy `buf.len()` bytes from user-space at `user_addr` into `buf`.
/// Returns `Ok(())` on success or `Err(())` if address translation fails.
pub fn copy_in(page_map: &PageMapper, user_addr: usize, buf: &mut [u8]) -> Result<(), ()> {
    let mut offset = 0;
    let mut user_ptr = user_addr;

    while offset < buf.len() {
        let page_offset = user_ptr & (memory::PAGE_SIZE - 1);
        let chunk_size = (buf.len() - offset).min(memory::PAGE_SIZE - page_offset);

        let phys_addr = page_map.virt_to_phys(user_ptr).ok_or(())?;

        unsafe {
            core::ptr::copy_nonoverlapping(
                phys_addr as *const u8,
                buf[offset..].as_mut_ptr(),
                chunk_size,
            );
        }

        user_ptr += chunk_size;
        offset += chunk_size;
    }

    Ok(())
}

/// Copy a null-terminated C string from user-space at `user_addr`.
/// Reads page by page, scanning for '\0'. Returns the string without
/// the null terminator. Fails if no null is found within `max_len` bytes
/// or if address translation fails.
pub fn copy_in_str(page_map: &PageMapper, user_addr: usize, max_len: usize) -> Result<String, ()> {
    let mut result = String::new();
    let mut user_ptr = user_addr;
    let mut scanned = 0;

    while scanned < max_len {
        let page_offset = user_ptr & (memory::PAGE_SIZE - 1);
        let chunk_size = (max_len - scanned).min(memory::PAGE_SIZE - page_offset);

        let phys_addr = page_map.virt_to_phys(user_ptr).ok_or(())?;

        let chunk = unsafe {
            core::slice::from_raw_parts(phys_addr as *const u8, chunk_size)
        };

        if let Some(null_pos) = chunk.iter().position(|&b| b == 0) {
            let s = core::str::from_utf8(&chunk[..null_pos]).map_err(|_| ())?;
            result.push_str(s);
            return Ok(result);
        }

        let s = core::str::from_utf8(chunk).map_err(|_| ())?;
        result.push_str(s);

        user_ptr += chunk_size;
        scanned += chunk_size;
    }

    Err(()) // no null terminator found within max_len
}
