//! ext2 inode table access and caching

use alloc::boxed::Box;
use alloc::sync::Arc;

use crate::bytes::ReadIntLe;
use crate::drivers::{BlockError, BLOCK_SIZE};
use crate::file::{FileType, Inode};
use crate::file::{S_IFBLK, S_IFCHR, S_IFDIR, S_IFREG, S_IFMT};

use super::ops::{EXT2_FILE_OPS, EXT2_INODE_OPS};
use super::superblock::Ext2SuperBlock;

const NDIR_BLOCKS: usize = 12;

// Inode structure offsets (within inode data)
const INODE_MODE_OFFSET: usize = 0;
const INODE_SIZE_OFFSET: usize = 4;
const INODE_BLOCKS_OFFSET: usize = 40;

/// Filesystem-specific data stored in each ext2 `Inode::fs_data`.
pub(super) struct Ext2InodeData {
    pub(super) sb: &'static Ext2SuperBlock,
    pub(super) blocks: [u32; 15],
}

impl Ext2SuperBlock {
    /// Read an inode from disk and return a fully constructed VFS inode.
    ///
    /// Requires `&'static self` so the superblock reference can be stored in the inode's `fs_data`.
    pub(super) fn read_inode(&'static self, inode_num: u32) -> Result<Arc<Inode>, BlockError> {
        let (sector, offset) = self.inode_location(inode_num)?;

        let buf = self.volume.as_ref().get_block(sector)?;

        let mode = buf.read_u16_le(offset + INODE_MODE_OFFSET);
        let size = buf.read_u32_le(offset + INODE_SIZE_OFFSET);
        let mut blocks = [0u32; 15];
        for i in 0..15 {
            blocks[i] = buf.read_u32_le(offset + INODE_BLOCKS_OFFSET + i * 4);
        }

        let file_type = match mode & S_IFMT {
            S_IFDIR => FileType::Directory,
            S_IFREG => FileType::RegularFile,
            S_IFCHR => FileType::CharDevice,
            S_IFBLK => FileType::BlockDevice,
            _       => FileType::RegularFile,
        };

        Ok(Arc::new(Inode {
            ino: inode_num as u64,
            file_type,
            len: size as usize,
            iops: &EXT2_INODE_OPS,
            fops: &EXT2_FILE_OPS,
            sb: Some(self),
            rdev: None,
            fs_data: Box::new(Ext2InodeData { sb: self, blocks }),
        }))
    }

    /// Return an inode by number, consulting the cache first.
    ///
    /// On a cache hit, moves the entry to the front of the LRU and returns it
    /// without a disk read. On a miss, reads from disk and inserts into the cache.
    pub(super) fn get_or_read_inode(&'static self, inode_num: u32) -> Result<Arc<Inode>, BlockError> {
        let cache = unsafe { &mut *self.inode_cache.get() };
        if let Some(arc) = cache.get(&inode_num) {
            return Ok(arc);
        }
        let inode = self.read_inode(inode_num)?;
        cache.insert(inode_num, Arc::clone(&inode));
        Ok(inode)
    }

    /// Resolve a logical block index to a physical block pointer.
    ///
    /// Handles direct blocks (0..12) and single-indirect (12..12+ptrs_per_block).
    /// Returns 0 for an unallocated (sparse) block or an out-of-range index.
    pub(super) fn resolve_block_ptr(
        &self,
        blocks: &[u32; 15],
        block_idx: usize,
    ) -> Result<u32, BlockError> {
        if block_idx < NDIR_BLOCKS {
            return Ok(blocks[block_idx]);
        }

        let block_size = self.block_size() as usize;
        let ptrs_per_block = block_size / 4;
        let indirect_idx = block_idx - NDIR_BLOCKS;
        if indirect_idx < ptrs_per_block {
            let indirect_ptr = blocks[NDIR_BLOCKS];
            if indirect_ptr == 0 {
                return Ok(0);
            }
            let buf = self.volume.as_ref().get_block(self.block_to_sector(indirect_ptr))?;
            return Ok(buf.read_u32_le(indirect_idx * 4));
        }

        let double_idx = indirect_idx - ptrs_per_block;
        if double_idx < ptrs_per_block * ptrs_per_block {
            let dindirect_ptr = blocks[NDIR_BLOCKS + 1];
            if dindirect_ptr == 0 {
                return Ok(0);
            }
            let l1_buf = self.volume.as_ref().get_block(self.block_to_sector(dindirect_ptr))?;
            let l1_ptr = l1_buf.read_u32_le((double_idx / ptrs_per_block) * 4);
            if l1_ptr == 0 {
                return Ok(0);
            }
            let l2_buf = self.volume.as_ref().get_block(self.block_to_sector(l1_ptr))?;
            return Ok(l2_buf.read_u32_le((double_idx % ptrs_per_block) * 4));
        }

        Ok(0) // triple indirect not supported
    }

    /// Calculate the disk sector and byte offset for a given inode number.
    fn inode_location(&self, inode_num: u32) -> Result<(u64, usize), BlockError> {
        if inode_num == 0 {
            return Err(BlockError::InvalidInput);
        }

        let group = ((inode_num - 1) / self.inodes_per_group) as usize;
        let local_index = (inode_num - 1) % self.inodes_per_group;

        if group >= self.groups.len() {
            return Err(BlockError::InvalidInput);
        }

        let inode_size = self.inode_size as u32;
        let block_size = self.block_size();
        let inode_table_block = self.groups[group].inode_table;
        let inodes_per_block = block_size / inode_size;
        let target_block = inode_table_block + local_index / inodes_per_block;
        let sector = (target_block as u64 * block_size as u64) / BLOCK_SIZE as u64;
        let offset = ((local_index % inodes_per_block) * inode_size) as usize;

        Ok((sector, offset))
    }
}
