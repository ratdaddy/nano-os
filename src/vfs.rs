//! Virtual File System operations.
//!
//! Provides the kernel's file operation API. The VFS caches a root inode
//! and uses inode operations for path traversal.

use alloc::string::String;
use alloc::vec::Vec;

use crate::file::{Error, File, Inode, SeekFrom};

static mut ROOT_INODE: Option<&'static dyn Inode> = None;

/// Initialize the VFS with a root inode.
pub fn init(root: &'static dyn Inode) {
    unsafe {
        ROOT_INODE = Some(root);
    }
}

/// Open a file by path.
pub fn vfs_open(path: &str) -> Result<File, Error> {
    let mut inode = unsafe { ROOT_INODE.ok_or(Error::InvalidInput)? };
    for component in path.split('/').filter(|s| !s.is_empty()) {
        inode = inode.lookup(component)?;
    }
    let fops = inode.file_ops();
    fops.open(inode)
}

/// Read directory entries.
///
/// Opens the directory and calls readdir on it.
/// Returns a vector of (name, size, is_dir) tuples.
pub fn vfs_readdir(path: &str) -> Result<Vec<(String, usize, bool)>, Error> {
    let mut file = vfs_open(path)?;
    let ops = file.fops;
    ops.readdir(&mut file)
}

/// Read from a file into a buffer.
/// Returns the number of bytes read (0 indicates EOF).
pub fn vfs_read(file: &mut File, buf: &mut [u8]) -> Result<usize, Error> {
    let ops = file.fops;  // Copy fat pointer (no borrow held)
    ops.read(file, buf)
}

/// Seek to a position in a file.
pub fn vfs_seek(file: &mut File, pos: SeekFrom) -> Result<(), Error> {
    let ops = file.fops;
    ops.seek(file, pos)
}

/// Read exactly `buf.len()` bytes from a file.
/// Returns an error if EOF is reached before the buffer is filled.
pub fn vfs_read_exact(file: &mut File, buf: &mut [u8]) -> Result<(), Error> {
    let mut remaining = buf;
    while !remaining.is_empty() {
        let n = vfs_read(file, remaining)?;
        if n == 0 {
            return Err(Error::UnexpectedEof);
        }
        remaining = &mut remaining[n..];
    }
    Ok(())
}

/// Read entire file into a string.
pub fn vfs_read_to_string(file: &mut File, out: &mut alloc::string::String) -> Result<(), Error> {
    let mut buf = [0u8; 256];
    loop {
        let len = vfs_read(file, &mut buf)?;
        if len == 0 {
            break;
        }
        let s = core::str::from_utf8(&buf[..len]).map_err(|_| Error::InvalidUtf8)?;
        out.push_str(s);
    }
    Ok(())
}

/// Write a buffer to a file.
/// Returns the number of bytes written.
pub fn vfs_write(file: &mut File, buf: &[u8]) -> Result<usize, Error> {
    let ops = file.fops;
    ops.write(file, buf)
}
