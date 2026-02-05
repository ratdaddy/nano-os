//! Virtual File System operations.
//!
//! Provides the kernel's file operation API. These functions copy the fops
//! pointer before calling, allowing &mut File to be passed to the ops method.

use crate::file::{Error, File, SeekFrom};

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

/// Write a buffer to a file.
/// Returns the number of bytes written.
pub fn vfs_write(file: &mut File, buf: &[u8]) -> Result<usize, Error> {
    let ops = file.fops;
    ops.write(file, buf)
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
