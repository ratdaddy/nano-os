//! ext2 filesystem implementation
//!
//! Read-only ext2 filesystem driver.

mod dir;
mod inode;
mod ops;
mod superblock;

pub use superblock::Ext2SuperBlock;

use crate::file::{Error, SuperBlock};
use crate::vfs::FileSystem;

/// ext2 filesystem type for VFS registration
pub struct Ext2FileSystem;

/// Global ext2 filesystem instance
pub static EXT2_FS: Ext2FileSystem = Ext2FileSystem;

impl FileSystem for Ext2FileSystem {
    fn name(&self) -> &'static str { "ext2" }

    fn mount(&self, source: Option<&str>) -> Result<&'static dyn SuperBlock, Error> {
        let source = source.ok_or(Error::InvalidInput)?;
        let inode = crate::vfs::vfs_lookup(source)?;
        let (major, minor) = inode.rdev.ok_or(Error::InvalidInput)?;
        let volume = crate::dev::blkdev_get(major, minor).map_err(|_| Error::NotFound)?;
        let sb = Ext2SuperBlock::new(volume).map_err(|_| Error::InvalidInput)?;
        Ok(sb)
    }
}
