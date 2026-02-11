//! Standard errno values for syscall error returns.
//!
//! These match Linux errno values for compatibility.

pub const ENOENT: isize = -2;  // No such file or directory
pub const EIO: isize = -5;     // I/O error
pub const EBADF: isize = -9;   // Bad file descriptor
pub const EFAULT: isize = -14; // Bad address
