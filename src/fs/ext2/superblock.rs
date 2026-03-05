//! ext2 superblock, group descriptors, and filesystem initialization

use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::str::from_utf8;

use crate::block::volume::{BlockVolume, CACHE_BLOCK_SIZE};
use crate::bytes::ReadIntLe;
use crate::collections::LruCache;
use crate::drivers::{BlockError, BLOCK_SIZE};
use crate::file::{Inode, SuperBlock};

pub(super) const INODE_CACHE_CAPACITY: usize = 32;
pub(super) const NDIR_BLOCKS: usize = 12;
pub(super) const MIN_BLOCK_SIZE: u32 = 1024;
pub(super) const ROOT_INODE: u32 = 2;

const GOOD_OLD_INODE_SIZE: u16 = 128;

// Superblock constants
const SUPERBLOCK_OFFSET: u64 = 1024;
const SUPERBLOCK_SECTOR: u64 = SUPERBLOCK_OFFSET / BLOCK_SIZE as u64;
const SUPER_MAGIC: u16 = 0xEF53;

// Superblock field offsets (within superblock data)
const SB_INODES_COUNT_OFFSET: usize = 0;
const SB_BLOCKS_COUNT_OFFSET: usize = 4;
const SB_LOG_BLOCK_SIZE_OFFSET: usize = 24;
const SB_BLOCKS_PER_GROUP_OFFSET: usize = 32;
const SB_INODES_PER_GROUP_OFFSET: usize = 40;
const MAGIC_OFFSET: usize = 56;
const SB_INODE_SIZE_OFFSET: usize = 88;
const VOLUME_LABEL_OFFSET: usize = 120;
const VOLUME_LABEL_LEN: usize = 16;

// Group descriptor constants
const GROUP_DESC_SIZE: usize = 32;
const GDT_1K_OFFSET: u64 = 2048;
const GD_INODE_TABLE_OFFSET: usize = 8;

/// ext2 superblock structure (per-mount instance)
///
/// Holds the mounted volume, parsed superblock fields, and cached group descriptors.
pub struct Ext2SuperBlock {
    pub(super) volume: Arc<dyn BlockVolume>,
    pub inodes_count: u32,
    pub blocks_count: u32,
    pub log_block_size: u32,
    pub blocks_per_group: u32,
    pub inodes_per_group: u32,
    pub inode_size: u16,
    volume_label: Option<String>,
    pub groups: Vec<Ext2GroupDesc>,
    root: Option<Arc<Inode>>,
    pub(super) inode_cache: UnsafeCell<LruCache<u32, Inode>>,
}

// Safety: ext2 is only accessed from a single thread; UnsafeCell is used in place
// of a mutex since the kernel has no concurrent ext2 callers at this point.
unsafe impl Sync for Ext2SuperBlock {}

impl Ext2SuperBlock {
    /// Create and fully initialise an Ext2SuperBlock from a BlockVolume.
    ///
    /// Reads superblock metadata, group descriptors, and the root inode.
    /// The superblock is heap-allocated and leaked so that inodes can hold
    /// `&'static` back-references to it in their `fs_data`.
    pub fn new(volume: Arc<dyn BlockVolume>) -> Result<&'static Self, BlockError> {
        let mut sb_box = Box::new(Self {
            volume,
            inodes_count: 0,
            blocks_count: 0,
            log_block_size: 0,
            blocks_per_group: 0,
            inodes_per_group: 0,
            inode_size: 0,
            volume_label: None,
            groups: Vec::new(),
            root: None,
            inode_cache: UnsafeCell::new(LruCache::new(INODE_CACHE_CAPACITY)),
        });

        sb_box.read_superblock_data()?;
        sb_box.read_group_descriptors()?;

        // Safety: The box is about to be leaked, making this &'static valid.
        // We retain mutable access through sb_box until Box::leak consumes it.
        let sb: &'static Self = unsafe { &*(sb_box.as_ref() as *const Self) };
        sb_box.root = Some(sb.get_or_read_inode(ROOT_INODE)?);

        Ok(Box::leak(sb_box))
    }

    /// Calculate the actual block size from the log value
    pub fn block_size(&self) -> u32 {
        MIN_BLOCK_SIZE << self.log_block_size
    }

    /// Convert an ext2 block number to the disk sector passed to `get_block`.
    ///
    /// `get_block` always reads `CACHE_BLOCK_SIZE` (4096) bytes starting at the
    /// returned sector. The file data occupies `buf[0..block_size]`.
    pub(super) fn block_to_sector(&self, block_num: u32) -> u64 {
        block_num as u64 * self.block_size() as u64 / BLOCK_SIZE as u64
    }

    /// Calculate number of block groups in the filesystem
    pub fn num_groups(&self) -> u32 {
        (self.blocks_count + self.blocks_per_group - 1) / self.blocks_per_group
    }

    /// Get the volume label
    pub fn volume_label(&self) -> Option<&str> {
        self.volume_label.as_deref()
    }

    /// Read and parse superblock data from volume
    fn read_superblock_data(&mut self) -> Result<(), BlockError> {
        let buf = self.volume.as_ref().get_block(SUPERBLOCK_SECTOR)?;

        let magic = buf.read_u16_le(MAGIC_OFFSET);
        if magic != SUPER_MAGIC {
            kprintln!("ext2: Invalid magic number {:#x} (expected {:#x})", magic, SUPER_MAGIC);
            return Err(BlockError::InvalidInput);
        }

        self.inodes_count = buf.read_u32_le(SB_INODES_COUNT_OFFSET);
        self.blocks_count = buf.read_u32_le(SB_BLOCKS_COUNT_OFFSET);
        self.log_block_size = buf.read_u32_le(SB_LOG_BLOCK_SIZE_OFFSET);
        self.blocks_per_group = buf.read_u32_le(SB_BLOCKS_PER_GROUP_OFFSET);
        self.inodes_per_group = buf.read_u32_le(SB_INODES_PER_GROUP_OFFSET);
        self.inode_size = buf.read_u16_le(SB_INODE_SIZE_OFFSET);
        // Revision 0 uses 128-byte inodes
        if self.inode_size == 0 {
            self.inode_size = GOOD_OLD_INODE_SIZE;
        }

        let label_bytes = &buf[VOLUME_LABEL_OFFSET..VOLUME_LABEL_OFFSET + VOLUME_LABEL_LEN];
        self.volume_label = if let Some(null_pos) = label_bytes.iter().position(|&b| b == 0) {
            if null_pos > 0 {
                from_utf8(&label_bytes[..null_pos]).ok().map(String::from)
            } else {
                None
            }
        } else {
            None
        };

        Ok(())
    }

    /// Read and parse block group descriptors
    ///
    /// GDT location depends on filesystem block size:
    /// - 1KB blocks: GDT at byte 2048
    /// - 2KB+ blocks: GDT at byte block_size
    fn read_group_descriptors(&mut self) -> Result<(), BlockError> {
        const MAX_GROUPS_IN_BUFFER: usize = CACHE_BLOCK_SIZE / GROUP_DESC_SIZE;

        let num_groups = self.num_groups();
        let block_size = self.block_size();

        if num_groups as usize > MAX_GROUPS_IN_BUFFER {
            kprintln!("ext2: ERROR - Filesystem has {} groups, but our 4KB buffer can only hold {} group descriptors",
                      num_groups, MAX_GROUPS_IN_BUFFER);
            return Err(BlockError::InvalidInput);
        }

        let gdt_byte_offset = if block_size == MIN_BLOCK_SIZE {
            GDT_1K_OFFSET
        } else {
            block_size as u64
        };

        let gdt_sector = gdt_byte_offset / BLOCK_SIZE as u64;

        let buf = self.volume.as_ref().get_block(gdt_sector)?;

        for i in 0..num_groups {
            let offset = i as usize * GROUP_DESC_SIZE;
            let inode_table = buf.read_u32_le(offset + GD_INODE_TABLE_OFFSET);
            self.groups.push(Ext2GroupDesc { inode_table });
        }

        Ok(())
    }
}

impl SuperBlock for Ext2SuperBlock {
    fn root_inode(&self) -> Arc<Inode> {
        Arc::clone(self.root.as_ref().expect("root not initialized; call new()"))
    }

    fn fs_type(&self) -> &'static str {
        "ext2"
    }
}

/// ext2 block group descriptor structure (in-memory, parsed from disk)
///
/// Only includes the field we actually use.
#[derive(Debug, Clone, Copy)]
pub struct Ext2GroupDesc {
    pub inode_table: u32,
}
