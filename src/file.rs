use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::Any;

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
    pub name: String,
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
    /// Inode for this file.
    pub inode: Arc<Inode>,
}

impl File {
    pub fn new(fops: &'static dyn FileOps, inode: Arc<Inode>) -> Self {
        Self {
            fops,
            offset: 0,
            inode,
        }
    }
}

/// File operations trait. Implementations must be Send + Sync since they're
/// stored as 'static references.
pub trait FileOps: Send + Sync {
    /// Open a file from an inode.
    fn open(&self, inode: Arc<Inode>) -> Result<File, Error> {
        Ok(File::new(inode.fops, inode))
    }

    fn read(&self, _file: &mut File, _buf: &mut [u8]) -> Result<usize, Error> {
        Err(Error::InvalidInput)
    }

    fn write(&self, _file: &mut File, _buf: &[u8]) -> Result<usize, Error> {
        Err(Error::InvalidInput)
    }

    fn seek(&self, file: &mut File, pos: SeekFrom) -> Result<(), Error> {
        let file_len = file.inode.len;
        file.offset = match pos {
            SeekFrom::Start(n)   => n.min(file_len),
            SeekFrom::Current(n) => {
                if n >= 0 {
                    file.offset.saturating_add(n as usize).min(file_len)
                } else {
                    file.offset.saturating_sub((-n) as usize)
                }
            }
        };
        Ok(())
    }

    /// Read directory entries (for directories).
    fn readdir(&self, _file: &mut File) -> Result<Vec<DirEntry>, Error> {
        Err(Error::InvalidInput) // Not a directory
    }
}

/// Inode operations trait — filesystem-specific directory operations.
///
/// Implementations must be Send + Sync since they're stored as 'static references.
pub trait InodeOps: Send + Sync {
    fn lookup(&self, _inode: &Arc<Inode>, _name: &str) -> Result<Arc<Inode>, Error> {
        Err(Error::InvalidInput)
    }
}

/// Universal inode struct shared across all filesystems.
///
/// Filesystem-specific data lives in `fs_data`, accessed via downcast.
/// `iops` and `fops` point to static singleton operation tables.
pub struct Inode {
    pub ino: u64,
    pub file_type: FileType,
    pub len: usize,
    pub iops: &'static dyn InodeOps,
    pub fops: &'static dyn FileOps,
    pub sb: Option<&'static dyn SuperBlock>,
    pub rdev: Option<(u32, u32)>,
    pub fs_data: Box<dyn Any + Send + Sync>,
}

/// Stable identity key for an inode (pointer address of the allocation).
pub fn inode_id(inode: &Arc<Inode>) -> usize {
    Arc::as_ptr(inode) as usize
}

/// SuperBlock trait — each filesystem provides one per mount.
pub trait SuperBlock: Send + Sync {
    fn root_inode(&self) -> Arc<Inode>;
    fn fs_type(&self) -> &'static str;
}
