#[derive(Debug, Clone, Copy)]
pub enum Error {
    UnexpectedEof,
    InvalidUtf8,
    InvalidInput,
    NotADirectory,
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    RegularFile,
    Directory,
    CharDevice,
}

pub struct DirEntry {
    pub name: alloc::string::String,
    pub file_type: FileType,
}

#[allow(dead_code)]
pub enum SeekFrom {
    Start(usize),
    Current(isize),
}

/// A file handle with position tracking and inode reference.
///
/// This is the kernel's internal representation of an open file.
/// Use vfs_read/vfs_write/vfs_seek to perform operations.
pub struct File {
    /// Static reference to file operations (copied, not borrowed, when calling)
    pub fops: &'static dyn FileOps,
    /// Current read/write position in the file
    pub offset: usize,
    /// Inode for this file. Currently optional for legacy device files (UART),
    /// but will become required when devices are integrated into VFS.
    pub inode: Option<&'static dyn Inode>,
}

impl File {
    /// Create a new File with the given FileOps.
    pub fn new(fops: &'static dyn FileOps) -> Self {
        Self {
            fops,
            offset: 0,
            inode: None,
        }
    }

    /// Create a new File with FileOps and an inode.
    pub fn with_inode(fops: &'static dyn FileOps, inode: &'static dyn Inode) -> Self {
        Self {
            fops,
            offset: 0,
            inode: Some(inode),
        }
    }
}

/// File operations trait. Implementations must be Send + Sync since they're
/// stored as 'static references.
pub trait FileOps: Send + Sync {
    /// Open a file from an inode.
    fn open(&self, inode: &'static dyn Inode) -> Result<File, Error> {
        Ok(File::with_inode(inode.file_ops(), inode))
    }

    fn read(&self, _file: &mut File, _buf: &mut [u8]) -> Result<usize, Error> {
        Err(Error::InvalidInput)
    }

    fn write(&self, _file: &mut File, _buf: &[u8]) -> Result<usize, Error> {
        Err(Error::InvalidInput)
    }

    fn seek(&self, _file: &mut File, _pos: SeekFrom) -> Result<(), Error> {
        Err(Error::InvalidInput)
    }

    /// Read directory entries (for directories).
    fn readdir(&self, _file: &mut File) -> Result<alloc::vec::Vec<DirEntry>, Error> {
        Err(Error::InvalidInput) // Not a directory
    }
}

/// Inode trait for filesystem-specific metadata and operations.
///
/// Implementations must be Send + Sync since they're stored as 'static references.
pub trait Inode: Send + Sync {
    /// Downcast to concrete type for filesystem-specific operations.
    fn as_any(&self) -> &dyn core::any::Any;

    /// Size of the file in bytes.
    fn len(&self) -> usize;

    /// Look up a child by name (for directories).
    /// Returns the child inode if found.
    fn lookup(&self, _name: &str) -> Result<&'static dyn Inode, Error> {
        Err(Error::InvalidInput) // Not a directory
    }

    /// Get the FileOps for this inode.
    fn file_ops(&self) -> &'static dyn FileOps;

    /// Return device major/minor numbers (for device nodes).
    fn rdev(&self) -> Option<(u32, u32)> {
        None
    }
}
