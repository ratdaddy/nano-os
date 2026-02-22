//! ext2 filesystem implementation
//!
//! Read-only ext2 filesystem driver.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::str::from_utf8;

use crate::block::volume::BlockVolume;
use crate::bytes::ReadIntLe;
use crate::drivers::{BlockError, BLOCK_SIZE};
use crate::file::{Error, Inode, SuperBlock};
use crate::vfs::FileSystem;

// =============================================================================
// Filesystem driver
// =============================================================================

/// ext2 filesystem type for VFS registration
pub struct Ext2FileSystem;

/// Global ext2 filesystem instance
pub static EXT2_FS: Ext2FileSystem = Ext2FileSystem;

impl FileSystem for Ext2FileSystem {
    fn name(&self) -> &'static str { "ext2" }
    fn requires_device(&self) -> bool { true }

    fn mount(&self) -> Result<&'static dyn SuperBlock, Error> {
        // TODO: Implement proper mount() with device registry (Phase 1.4)
        // This requires:
        // - Device registry to map (major, minor) -> Arc<dyn BlockVolume>
        // - Parsing source device path (e.g., "/dev/sda1")
        // - Looking up device in registry
        // - Creating Ext2SuperBlock from the volume
        // - Storing in static storage and returning reference
        Err(Error::NotFound)
    }
}

// Static buffer for ext2 block I/O (4KB) - must be page-aligned for DMA
//
// Page alignment (4096) ensures buffer doesn't cross physical page boundaries.
// Used for reading superblock, group descriptors, inodes, and directory entries.
//
// Block size strategy:
// - Superblock: Always 1024 bytes (ext2 spec), so we read exactly 1KB
// - Other structures: Read full 4KB even if filesystem uses smaller blocks (1KB/2KB)
//   Modern ext2 filesystems use 4KB blocks anyway. For smaller block sizes,
//   this reads more than needed but still works correctly and simplifies the code.
#[repr(C, align(4096))]
struct Ext2BlockBuffer([u8; IO_BUFFER_SIZE]);

static mut IO_BUFFER: Ext2BlockBuffer = Ext2BlockBuffer([0; IO_BUFFER_SIZE]);

// Superblock constants
const SUPERBLOCK_OFFSET: u64 = 1024;
#[allow(dead_code)]
const SUPERBLOCK_SIZE: usize = 1024;
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

// Block and inode constants
const MIN_BLOCK_SIZE: u32 = 1024;
const GOOD_OLD_INODE_SIZE: u16 = 128;
const NDIR_BLOCKS: usize = 12;
const IO_BUFFER_SIZE: usize = 4096;

// Group descriptor constants
const GROUP_DESC_SIZE: usize = 32;
const GDT_1K_OFFSET: u64 = 2048;
const GD_INODE_TABLE_OFFSET: usize = 8;

// File type masks (mode field)
const S_IFMT: u16 = 0xF000;
const S_IFDIR: u16 = 0x4000;
const S_IFREG: u16 = 0x8000;
#[allow(dead_code)]
const S_IFCHR: u16 = 0x2000;
#[allow(dead_code)]
const S_IFBLK: u16 = 0x6000;

// Inode structure offsets (within inode data)
const INODE_MODE_OFFSET: usize = 0;
const INODE_SIZE_OFFSET: usize = 4;
const INODE_BLOCKS_OFFSET: usize = 40;

// Directory entry structure offsets
const DIR_ENTRY_INODE_OFFSET: usize = 0;
const DIR_ENTRY_REC_LEN_OFFSET: usize = 4;
const DIR_ENTRY_NAME_LEN_OFFSET: usize = 6;
#[allow(dead_code)]
const DIR_ENTRY_FILE_TYPE_OFFSET: usize = 7;
const DIR_ENTRY_NAME_OFFSET: usize = 8;

/// ext2 superblock structure (per-mount instance)
///
/// Holds the mounted volume, parsed superblock fields, and cached group descriptors.
pub struct Ext2SuperBlock {
    volume: Arc<dyn BlockVolume>,
    pub inodes_count: u32,
    pub blocks_count: u32,
    pub log_block_size: u32,
    pub blocks_per_group: u32,
    pub inodes_per_group: u32,
    pub inode_size: u16,
    volume_label: Option<String>,
    groups: Vec<Ext2GroupDesc>,
}

impl Ext2SuperBlock {
    /// Create a new Ext2SuperBlock by reading from a BlockVolume
    pub fn new(volume: Arc<dyn BlockVolume>) -> Result<Self, BlockError> {
        // Initialize with placeholder values
        let mut sb = Self {
            volume,
            inodes_count: 0,
            blocks_count: 0,
            log_block_size: 0,
            blocks_per_group: 0,
            inodes_per_group: 0,
            inode_size: 0,
            volume_label: None,
            groups: Vec::new(),
        };

        // Populate superblock fields from disk
        sb.read_superblock_data()?;

        // Read group descriptors
        sb.read_group_descriptors()?;

        Ok(sb)
    }

    /// Calculate the actual block size from the log value
    pub fn block_size(&self) -> u32 {
        MIN_BLOCK_SIZE << self.log_block_size
    }

    /// Calculate number of block groups in the filesystem
    pub fn num_groups(&self) -> u32 {
        (self.blocks_count + self.blocks_per_group - 1) / self.blocks_per_group
    }

    /// Get the volume label
    pub fn volume_label(&self) -> Option<&str> {
        self.volume_label.as_deref()
    }

    /// Get a reference to the underlying volume
    pub fn volume(&self) -> &dyn BlockVolume {
        self.volume.as_ref()
    }

    /// Get the group descriptors
    pub fn groups(&self) -> &[Ext2GroupDesc] {
        &self.groups
    }

    /// Read and parse superblock data from volume
    fn read_superblock_data(&mut self) -> Result<(), BlockError> {
        let buf = unsafe {
            let buf = &raw mut IO_BUFFER.0;
            let buf = &mut *buf;
            let superblock_slice = &mut buf[..SUPERBLOCK_SIZE];
            self.volume.as_ref().read_blocks(SUPERBLOCK_SECTOR, superblock_slice)?;
            superblock_slice
        };

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
        const MAX_GROUPS_IN_BUFFER: usize = IO_BUFFER_SIZE / GROUP_DESC_SIZE;

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

        let buf = unsafe {
            let buf = &raw mut IO_BUFFER.0;
            let buf = &mut *buf;
            self.volume.as_ref().read_blocks(gdt_sector, buf)?;
            buf as &[u8]
        };

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
        todo!("root_inode() requires inode lifetime management - Phase 2")
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

/// ext2 inode structure (in-memory, parsed from disk)
///
/// Only includes the fields we actually use.
#[derive(Debug)]
pub struct Ext2Inode {
    pub mode: u16,
    pub size: u32,
    pub blocks: [u32; 15],
}

impl Ext2Inode {
    /// Get file size in bytes
    pub fn len(&self) -> usize {
        self.size as usize
    }

    /// Check if this inode represents a directory
    pub fn is_dir(&self) -> bool {
        (self.mode & S_IFMT) == S_IFDIR
    }

    /// Check if this inode represents a regular file
    #[allow(dead_code)]
    pub fn is_reg(&self) -> bool {
        (self.mode & S_IFMT) == S_IFREG
    }
}

/// Read an inode by inode number
///
/// # Arguments
/// * `volume` - The volume to read from
/// * `sb` - The superblock (for inodes_per_group and block_size)
/// * `groups` - The group descriptors (for inode table locations)
/// * `inode_num` - The inode number (1-indexed, inode 2 = root)
///
/// # Returns
/// Parsed inode structure
pub fn read_inode(
    volume: &dyn BlockVolume,
    sb: &Ext2SuperBlock,
    groups: &[Ext2GroupDesc],
    inode_num: u32,
) -> Result<Ext2Inode, BlockError> {
    if inode_num == 0 {
        return Err(BlockError::InvalidInput);
    }

    let group = ((inode_num - 1) / sb.inodes_per_group) as usize;
    let local_index = (inode_num - 1) % sb.inodes_per_group;

    if group >= groups.len() {
        return Err(BlockError::InvalidInput);
    }

    let inode_table_block = groups[group].inode_table;
    let inode_size = sb.inode_size as u32;
    let block_size = sb.block_size();
    let inodes_per_block = block_size / inode_size;
    let block_offset = local_index / inodes_per_block;
    let inode_index_in_block = local_index % inodes_per_block;
    let target_block = inode_table_block + block_offset;
    let sector = (target_block as u64 * block_size as u64) / BLOCK_SIZE as u64;

    let buf = unsafe {
        let buf = &raw mut IO_BUFFER.0;
        let buf = &mut *buf;
        volume.read_blocks(sector, buf)?;
        buf as &[u8]
    };

    let offset_in_block = (inode_index_in_block * inode_size) as usize;
    let mode = buf.read_u16_le(offset_in_block + INODE_MODE_OFFSET);
    let size = buf.read_u32_le(offset_in_block + INODE_SIZE_OFFSET);

    let mut blocks = [0u32; 15];
    for i in 0..15 {
        blocks[i] = buf.read_u32_le(offset_in_block + INODE_BLOCKS_OFFSET + i * 4);
    }

    Ok(Ext2Inode { mode, size, blocks })
}

/// Parse directory entries from a directory inode
///
/// Returns a list of (inode_num, name) pairs for all entries in the directory.
/// Directory entries are stored in the data blocks pointed to by the inode's blocks array.
///
/// # Arguments
/// * `volume` - The volume to read from
/// * `sb` - The superblock (for block size)
/// * `inode` - The directory inode to read entries from
///
/// # Returns
/// Vector of (inode_number, filename) pairs
pub fn read_dir_entries(
    volume: &dyn BlockVolume,
    sb: &Ext2SuperBlock,
    inode: &Ext2Inode,
) -> Result<Vec<(u32, String)>, BlockError> {

    if !inode.is_dir() {
        return Err(BlockError::InvalidInput);
    }

    let block_size = sb.block_size();
    let mut entries = Vec::new();
    let mut bytes_read = 0u32;

    // Only direct blocks supported for now
    for block_ptr in inode.blocks.iter().take(NDIR_BLOCKS) {
        if *block_ptr == 0 || bytes_read >= inode.size {
            break;
        }

        let block_byte_offset = *block_ptr as u64 * block_size as u64;
        let sector = block_byte_offset / BLOCK_SIZE as u64;

        let block_buf = unsafe {
            let buf = &raw mut IO_BUFFER.0;
            let buf = &mut *buf;
            volume.read_blocks(sector, buf)?;
            buf as &[u8]
        };

        let mut offset = 0;
        while offset < block_size as usize && bytes_read < inode.size {
            if offset + DIR_ENTRY_NAME_OFFSET > block_buf.len() {
                break;
            }

            let entry_inode = block_buf.read_u32_le(offset + DIR_ENTRY_INODE_OFFSET);
            let rec_len = block_buf.read_u16_le(offset + DIR_ENTRY_REC_LEN_OFFSET);
            let name_len = block_buf[offset + DIR_ENTRY_NAME_LEN_OFFSET];

            if rec_len == 0 {
                break;
            }

            if entry_inode != 0 && name_len > 0 {
                let name_start = offset + DIR_ENTRY_NAME_OFFSET;
                let name_end = name_start + name_len as usize;

                if name_end <= block_buf.len() {
                    if let Ok(name) = core::str::from_utf8(&block_buf[name_start..name_end]) {
                        entries.push((entry_inode, String::from(name)));
                    }
                }
            }

            offset += rec_len as usize;
            bytes_read += rec_len as u32;
        }
    }

    Ok(entries)
}

/// Look up a filename in a directory
///
/// # Arguments
/// * `volume` - The volume to read from
/// * `sb` - The superblock
/// * `groups` - The group descriptors
/// * `parent_inode` - The directory inode to search in
/// * `name` - The filename to find
///
/// # Returns
/// The inode number of the found file, or error if not found
pub fn lookup_entry(
    volume: &dyn BlockVolume,
    sb: &Ext2SuperBlock,
    parent_inode: &Ext2Inode,
    name: &str,
) -> Result<u32, BlockError> {
    let entries = read_dir_entries(volume, sb, parent_inode)?;

    for (inode_num, entry_name) in entries {
        if entry_name == name {
            return Ok(inode_num);
        }
    }

    Err(BlockError::InvalidInput)
}

/// Inspect an ext2 filesystem and display its structure
///
/// Reads the superblock, group descriptors, root directory, and tests lookup.
/// Used for development and debugging.
pub fn inspect_ext2(volume: Arc<dyn BlockVolume>) {
    kprintln!("\nReading ext2 filesystem\n");

    // Step 1: Create superblock (reads superblock and group descriptors)
    let sb = match Ext2SuperBlock::new(volume) {
        Ok(sb) => {
            // Print superblock info
            if let Some(label) = sb.volume_label() {
                kprintln!("ext2: {} blocks ({} bytes), {} inodes, {} groups ('{}')",
                         sb.blocks_count, sb.block_size(), sb.inodes_count, sb.num_groups(), label);
            } else {
                kprintln!("ext2: {} blocks ({} bytes), {} inodes, {} groups",
                         sb.blocks_count, sb.block_size(), sb.inodes_count, sb.num_groups());
            }

            // Print group descriptor info
            for (i, group) in sb.groups.iter().enumerate() {
                kprintln!("  Group {}: inode_table={}", i, group.inode_table);
            }

            sb
        }
        Err(e) => {
            kprintln!("ext2: Failed to read superblock: {:?}", e);
            return;
        }
    };

    // Step 3: Read root inode (inode #2 is always the root directory)
    let root_inode = match read_inode(sb.volume(), &sb, sb.groups(), 2) {
        Ok(root_inode) => root_inode,
        Err(e) => {
            kprintln!("ext2: Failed to read root inode: {:?}", e);
            return;
        }
    };

    // Step 4: List root directory contents
    let entries = match read_dir_entries(sb.volume(), &sb, &root_inode) {
        Ok(entries) => {
            kprintln!("ext2: Root directory (inode 2): {} bytes, {} entries",
                     root_inode.len(), entries.len());
            for (inode_num, name) in &entries {
                kprintln!("  {} (inode {})", name, inode_num);
            }
            entries
        }
        Err(e) => {
            kprintln!("ext2: Failed to read directory entries: {:?}", e);
            return;
        }
    };

    // Step 5: Test lookup functionality
    if let Some((target_ino, target_name)) = entries.iter()
        .find(|(_, name)| name != "." && name != "..") {

        if let Ok(found_ino) = lookup_entry(sb.volume(), &sb, &root_inode, target_name) {
            if found_ino == *target_ino {
                if let Ok(inode) = read_inode(sb.volume(), &sb, sb.groups(), found_ino) {
                    let file_type = if inode.is_dir() { "directory" } else { "file" };
                    kprintln!("ext2: '{}' lookup: inode {}, {}, {} bytes",
                             target_name, found_ino, file_type, inode.len());
                }
            }
        }
    }
}

