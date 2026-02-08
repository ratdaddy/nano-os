//! Synthetic /proc filesystem.

use alloc::boxed::Box;
use core::any::Any;

use crate::file::{DirEntry, Error, File, FileOps, FileType, Inode};
use crate::vfs::{FileSystem, SuperBlock};

// =============================================================================
// Inode
// =============================================================================

struct ProcfsInode;

static PROCFS_FILE_OPS: ProcfsFileOps = ProcfsFileOps;

impl Inode for ProcfsInode {
    fn as_any(&self) -> &dyn Any { self }
    fn file_type(&self) -> FileType { FileType::Directory }
    fn len(&self) -> usize { 0 }
    fn file_ops(&self) -> &'static dyn FileOps { &PROCFS_FILE_OPS }
}

// =============================================================================
// FileOps
// =============================================================================

struct ProcfsFileOps;

impl FileOps for ProcfsFileOps {
    fn readdir(&self, _file: &mut File) -> Result<alloc::vec::Vec<DirEntry>, Error> {
        Ok(alloc::vec::Vec::new())
    }
}

// =============================================================================
// SuperBlock
// =============================================================================

struct ProcfsSuperBlock {
    root: &'static ProcfsInode,
}

impl SuperBlock for ProcfsSuperBlock {
    fn root_inode(&self) -> &'static dyn Inode { self.root }
    fn fs_type(&self) -> &'static str { "proc" }
}

// =============================================================================
// Filesystem driver
// =============================================================================

pub struct ProcfsType;

impl FileSystem for ProcfsType {
    fn name(&self) -> &'static str { "proc" }
    fn mount(&self) -> Result<&'static dyn SuperBlock, Error> {
        Ok(new())
    }
}

pub static PROCFS_TYPE: ProcfsType = ProcfsType;

/// Create a new procfs SuperBlock.
pub fn new() -> &'static dyn SuperBlock {
    let root = Box::leak(Box::new(ProcfsInode));
    Box::leak(Box::new(ProcfsSuperBlock { root }))
}
