#[derive(Debug, Clone, Copy)]
pub enum Error {
    UnexpectedEof,
    InvalidUtf8,
    InvalidInput,
}

#[allow(dead_code)]
pub enum SeekFrom {
    Start(usize),
    Current(isize),
}

/// A file handle with position tracking and optional private data.
///
/// This is the kernel's internal representation of an open file.
/// Use vfs_read/vfs_write/vfs_seek to perform operations.
pub struct File {
    /// Static reference to file operations (copied, not borrowed, when calling)
    pub fops: &'static dyn FileOps,
    /// Current read/write position in the file
    pub offset: usize,
    /// Private data pointer (e.g., initramfs file content)
    pub data: *const u8,
    /// Length of private data
    pub data_len: usize,
}

impl File {
    /// Create a new File with the given FileOps.
    pub fn new(fops: &'static dyn FileOps) -> Self {
        Self {
            fops,
            offset: 0,
            data: core::ptr::null(),
            data_len: 0,
        }
    }

    /// Create a new File with FileOps and private data (for initramfs files).
    pub fn with_data(fops: &'static dyn FileOps, data: &'static [u8]) -> Self {
        Self {
            fops,
            offset: 0,
            data: data.as_ptr(),
            data_len: data.len(),
        }
    }
}

/// File operations trait. Implementations must be Send + Sync since they're
/// stored as 'static references.
pub trait FileOps: Send + Sync {
    fn read(&self, _file: &mut File, _buf: &mut [u8]) -> Result<usize, Error> {
        Err(Error::InvalidInput)
    }

    fn write(&self, _file: &mut File, _buf: &[u8]) -> Result<usize, Error> {
        Err(Error::InvalidInput)
    }

    fn seek(&self, _file: &mut File, _pos: SeekFrom) -> Result<(), Error> {
        Err(Error::InvalidInput)
    }
}
