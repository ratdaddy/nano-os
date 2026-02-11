use super::errno::{EBADF, EFAULT, EIO, ENOENT};
use super::uaccess;
use crate::file::{FileType, inode_id};
use crate::process;
use crate::thread;
use crate::vfs;

/// sys_write(fd, buf, count) -> bytes written or error
///
/// Copies user buffer into kernel space, then writes to the file.
pub fn sys_write(fd: usize, buf: *const u8, count: usize) -> isize {
    let ctx = process::Context::current();

    // Look up fd in process file table
    let file = match ctx.files.get_mut(fd) {
        Some(Some(f)) => f,
        _ => return EBADF,
    };

    let mut kbuf = alloc::vec![0u8; count];
    if uaccess::copy_in(&ctx.page_map, buf as usize, &mut kbuf).is_err() {
        return EFAULT;
    }

    let written = match vfs::vfs_write(file, &kbuf) {
        Ok(n) => n,
        Err(_) => return EIO,
    };

    unsafe {
        thread::yield_now();
    }

    written as isize
}

/// Syscall write wrapper - extracts args from trap frame and calls sys_write
pub fn write(tf: &mut types::ProcessTrapFrame) {
    let fd = tf.registers.a0;
    let buf = tf.registers.a1 as *const u8;
    let count = tf.registers.a2;

    tf.registers.a0 = sys_write(fd, buf, count) as usize;
}

/// Syscall openat
pub fn openat(tf: &mut types::ProcessTrapFrame) {
    let ctx = process::Context::current();

    let _dirfd = tf.registers.a0 as i64;
    let pathname = tf.registers.a1 as usize;
    let _flags = tf.registers.a2;
    let _mode = tf.registers.a3;

    let path = match uaccess::copy_in_str(&ctx.page_map, pathname, 256) {
        Ok(path) => path,
        Err(_) => {
            #[cfg(feature = "trace_syscalls")]
            println!("[openat]: dirfd: {}, error reading pathname from user-space", _dirfd);
            tf.registers.a0 = EFAULT as usize;
            return;
        }
    };

    #[cfg(feature = "trace_syscalls")]
    println!("[openat]: dirfd: {}, path: {}, flags: {:#x}, mode: {}", _dirfd, path, _flags, _mode);

    let file = match vfs::vfs_open(&path) {
        Ok(f) => f,
        Err(_) => {
            tf.registers.a0 = ENOENT as usize;
            return;
        }
    };

    // Allocate fd: reuse first None slot, or push a new entry
    let fd = match ctx.files.iter().position(|slot| slot.is_none()) {
        Some(i) => {
            ctx.files[i] = Some(file);
            i
        }
        None => {
            ctx.files.push(Some(file));
            ctx.files.len() - 1
        }
    };

    #[cfg(feature = "trace_syscalls")]
    println!("  fd: {}", fd);
    tf.registers.a0 = fd;
}

/// Syscall fcntl - no-op for now
pub fn fcntl(tf: &mut types::ProcessTrapFrame) {
    let _fd = tf.registers.a0;
    let _cmd = tf.registers.a1;
    let _arg = tf.registers.a2;

    #[cfg(feature = "trace_syscalls")]
    println!("[fcntl]: fd: {}, cmd: {}, arg: {}", _fd, _cmd, _arg);
}

/// Syscall newfstat
#[repr(C)]
struct Stat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_mode: u32,
    pub st_nlink: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub st_rdev: u64,
    pub __pad1: u64,
    pub st_size: i64,
    pub st_blksize: i32,
    pub __pad2: i32,
    pub st_blocks: i64,
    pub st_atime: i64,
    pub st_atime_nsec: u64,
    pub st_mtime: i64,
    pub st_mtime_nsec: u64,
    pub st_ctime: i64,
    pub st_ctime_nsec: u64,
    pub __unused4: u32,
    pub __unused5: u32,
}

pub fn newfstat(tf: &mut types::ProcessTrapFrame) {
    let fd = tf.registers.a0;
    let statbuf = tf.registers.a1 as usize;

    let ctx = process::Context::current();

    let file = match ctx.files.get(fd) {
        Some(Some(f)) => f,
        _ => {
            tf.registers.a0 = EBADF as usize;
            return;
        }
    };

    let inode = file.inode;

    let mode_type = match inode.file_type() {
        FileType::RegularFile => 0o100000,
        FileType::Directory   => 0o040000,
        FileType::CharDevice  => 0o020000,
    };

    let mut stat: Stat = unsafe { core::mem::zeroed() };
    stat.st_ino = inode_id(inode) as u64;
    stat.st_mode = mode_type | 0o444;
    stat.st_nlink = 1;
    stat.st_size = inode.len() as i64;
    stat.st_blksize = 4096;
    if let Some((major, minor)) = inode.rdev() {
        stat.st_rdev = ((major as u64) << 8) | (minor as u64);
    }

    #[cfg(feature = "trace_syscalls")]
    println!("[newfstat]: fd: {}, ino: {:#x}, mode: {:#o}, size: {}",
        fd, stat.st_ino, stat.st_mode, stat.st_size);

    let bytes = unsafe {
        core::slice::from_raw_parts(&stat as *const Stat as *const u8, core::mem::size_of::<Stat>())
    };
    if uaccess::copy_out(&ctx.page_map, statbuf, bytes).is_err() {
        tf.registers.a0 = EFAULT as usize;
        return;
    }

    tf.registers.a0 = 0;
}

/// Syscall read
pub fn read(tf: &mut types::ProcessTrapFrame) {
    let fd = tf.registers.a0;
    let buf = tf.registers.a1 as usize;
    let size = tf.registers.a2;

    #[cfg(feature = "trace_syscalls")]
    println!("[read]: fd: {}, buf: {:#x}, size: {}", fd, buf, size);

    let ctx = process::Context::current();

    let file = match ctx.files.get_mut(fd) {
        Some(Some(f)) => f,
        _ => {
            tf.registers.a0 = EBADF as usize;
            return;
        }
    };

    let mut kbuf = alloc::vec![0u8; size];
    let n = match vfs::vfs_read(file, &mut kbuf) {
        Ok(n) => n,
        Err(_) => {
            tf.registers.a0 = EIO as usize;
            return;
        }
    };

    if n > 0 {
        if uaccess::copy_out(&ctx.page_map, buf, &kbuf[..n]).is_err() {
            tf.registers.a0 = EFAULT as usize;
            return;
        }
    }

    tf.registers.a0 = n;
}

/// Syscall close
pub fn close(tf: &mut types::ProcessTrapFrame) {
    let fd = tf.registers.a0;

    #[cfg(feature = "trace_syscalls")]
    println!("[close]: fd: {}", fd);

    let ctx = process::Context::current();
    match ctx.files.get_mut(fd) {
        Some(slot @ Some(_)) => {
            *slot = None;
            tf.registers.a0 = 0;
        }
        _ => {
            tf.registers.a0 = EBADF as usize;
        }
    }
}
