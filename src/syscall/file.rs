use super::errno::{EBADF, EFAULT, EIO};
use crate::memory;
use crate::process;
use crate::thread;
use crate::vfs;

/// sys_write(fd, buf, count) -> bytes written or error
///
/// Handles buffers that span page boundaries by writing in chunks.
pub fn sys_write(fd: usize, buf: *const u8, count: usize) -> isize {
    let ctx = process::Context::current();

    // Look up fd in process file table
    let file = match ctx.files.get_mut(fd) {
        Some(Some(f)) => f,
        _ => return EBADF,
    };

    let mut written = 0usize;
    let mut remaining = count;
    let mut user_ptr = buf as usize;

    while remaining > 0 {
        // Calculate bytes until end of current page
        let page_offset = user_ptr & (memory::PAGE_SIZE - 1);
        let chunk_size = remaining.min(memory::PAGE_SIZE - page_offset);

        // Translate user virtual address to physical address
        let phys_addr = match ctx.page_map.virt_to_phys(user_ptr) {
            Some(addr) => addr,
            None => return EFAULT,
        };

        let data = unsafe { core::slice::from_raw_parts(phys_addr as *const u8, chunk_size) };

        match vfs::vfs_write(file, data) {
            Ok(n) => written += n,
            Err(_) => return EIO,
        }

        user_ptr += chunk_size;
        remaining -= chunk_size;
    }

    unsafe {
        thread::yield_now();
    }

    written as isize
}

/// Syscall wrapper - extracts args from trap frame and calls sys_write
pub fn write(tf: &mut types::ProcessTrapFrame) {
    let fd = tf.registers.a0;
    let buf = tf.registers.a1 as *const u8;
    let count = tf.registers.a2;

    tf.registers.a0 = sys_write(fd, buf, count) as usize;
}
