//! ext2 filesystem implementation
//!
//! Read-only ext2 filesystem driver.

use alloc::string::String;
use alloc::vec::Vec;
use core::str::from_utf8;

use crate::block::volume::BlockVolume;
use crate::bytes::ReadIntLe;
use crate::drivers::{BlockError, BLOCK_SIZE};

// Superblock constants
const SUPERBLOCK_OFFSET: u64 = 1024;  // Superblock always starts at byte 1024
#[allow(dead_code)]
const SUPERBLOCK_SIZE: usize = 1024;  // Superblock is 1024 bytes
const SUPERBLOCK_SECTOR: u64 = SUPERBLOCK_OFFSET / BLOCK_SIZE as u64;  // Sector 2 for 512-byte blocks
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
const MIN_BLOCK_SIZE: u32 = 1024;  // Minimum block size
const GOOD_OLD_INODE_SIZE: u16 = 128;  // Default inode size for revision 0
const NDIR_BLOCKS: usize = 12;  // Number of direct block pointers
const IO_BUFFER_SIZE: usize = 4096;  // Size of I/O buffer (page-aligned for DMA)

// Group descriptor constants
const GROUP_DESC_SIZE: usize = 32;  // Size of group descriptor structure
const GDT_1K_OFFSET: u64 = 2048;  // GDT location for 1KB block filesystems
const GD_INODE_TABLE_OFFSET: usize = 8;  // Inode table block number offset

// File type masks (mode field)
const S_IFMT: u16 = 0xF000;   // File type mask
const S_IFDIR: u16 = 0x4000;  // Directory
const S_IFREG: u16 = 0x8000;  // Regular file
#[allow(dead_code)]
const S_IFCHR: u16 = 0x2000;  // Character device
#[allow(dead_code)]
const S_IFBLK: u16 = 0x6000;  // Block device

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

/// ext2 superblock structure (in-memory, parsed from disk)
///
/// Only includes the fields we actually use.
#[derive(Debug)]
pub struct Ext2SuperBlock {
    pub inodes_count: u32,      // Total inodes (used in printout)
    pub blocks_count: u32,      // Total blocks (used in num_groups(), printout)
    pub log_block_size: u32,    // Block size calculation (used in block_size())
    pub blocks_per_group: u32,  // Blocks per group (used in num_groups(), printout)
    pub inodes_per_group: u32,  // Inodes per group (used in read_inode(), printout)
    pub inode_size: u16,        // Inode size (used in read_inode(), printout)
}

impl Ext2SuperBlock {
    /// Calculate the actual block size from the log value
    pub fn block_size(&self) -> u32 {
        MIN_BLOCK_SIZE << self.log_block_size
    }

    /// Calculate number of block groups in the filesystem
    pub fn num_groups(&self) -> u32 {
        (self.blocks_count + self.blocks_per_group - 1) / self.blocks_per_group
    }
}

/// ext2 block group descriptor structure (in-memory, parsed from disk)
///
/// Only includes the field we actually use.
#[derive(Debug, Clone, Copy)]
pub struct Ext2GroupDesc {
    pub inode_table: u32,  // Block number of first inode table block (used in read_inode)
}

// Static buffer for ext2 block I/O (4KB) - must be page-aligned for DMA
// Page alignment (4096) ensures buffer doesn't cross physical page boundaries
// Used for reading superblock, group descriptors, inodes, and directory entries
#[repr(C, align(4096))]
struct Ext2BlockBuffer([u8; IO_BUFFER_SIZE]);

static mut IO_BUFFER: Ext2BlockBuffer = Ext2BlockBuffer([0; IO_BUFFER_SIZE]);

/// Read and parse the ext2 superblock from a volume
///
/// The ext2 superblock is always 1024 bytes starting at byte offset 1024.
/// With 512-byte blocks, this means sectors 2-3 contain the superblock.
/// We read 8 sectors (4096 bytes) starting at sector 2, which includes the
/// full superblock plus additional data we can ignore.
pub fn read_superblock(volume: &dyn BlockVolume) -> Result<Ext2SuperBlock, BlockError> {
    // Verify our assumption about block size
    assert_eq!(volume.block_size(), BLOCK_SIZE as u32, "Volume block size mismatch");

    // Read 8 sectors (4096 bytes) starting at sector 2
    // This gives us the full superblock (1024 bytes at start of buffer)
    let buf = unsafe {
        let buf = &raw mut IO_BUFFER.0;
        let buf = &mut *buf;
        volume.read_blocks(SUPERBLOCK_SECTOR, buf)?;
        buf
    };

    // Check magic number first
    let magic = buf.read_u16_le(MAGIC_OFFSET);

    if magic != SUPER_MAGIC {
        kprintln!("ext2: Invalid magic number {:#x} (expected {:#x})", magic, SUPER_MAGIC);
        return Err(BlockError::InvalidInput);
    }

    // Parse only the fields we actually use
    let inodes_count = buf.read_u32_le(SB_INODES_COUNT_OFFSET);
    let blocks_count = buf.read_u32_le(SB_BLOCKS_COUNT_OFFSET);
    let log_block_size = buf.read_u32_le(SB_LOG_BLOCK_SIZE_OFFSET);
    let blocks_per_group = buf.read_u32_le(SB_BLOCKS_PER_GROUP_OFFSET);
    let inodes_per_group = buf.read_u32_le(SB_INODES_PER_GROUP_OFFSET);
    let inode_size = buf.read_u16_le(SB_INODE_SIZE_OFFSET);
    // Default to 128 if zero (revision 0 always uses 128-byte inodes)
    let inode_size = if inode_size == 0 { GOOD_OLD_INODE_SIZE } else { inode_size };

    let sb = Ext2SuperBlock {
        inodes_count,
        blocks_count,
        log_block_size,
        blocks_per_group,
        inodes_per_group,
        inode_size,
    };

    // Read volume label
    let label_bytes = &buf[VOLUME_LABEL_OFFSET..VOLUME_LABEL_OFFSET + VOLUME_LABEL_LEN];
    let label = if let Some(null_pos) = label_bytes.iter().position(|&b| b == 0) {
        if null_pos > 0 {
            from_utf8(&label_bytes[..null_pos]).ok()
        } else {
            None
        }
    } else {
        None
    };

    // Condensed output: key facts on one line
    if let Some(label) = label {
        kprintln!("ext2: {} blocks ({} bytes), {} inodes, {} groups ('{}')",
                 sb.blocks_count, sb.block_size(), sb.inodes_count, sb.num_groups(), label);
    } else {
        kprintln!("ext2: {} blocks ({} bytes), {} inodes, {} groups",
                 sb.blocks_count, sb.block_size(), sb.inodes_count, sb.num_groups());
    }

    Ok(sb)
}

/// Read and parse block group descriptors
///
/// The GDT location depends on the filesystem block size:
/// - **1KB blocks**: Superblock at block 1, GDT at block 2 (byte 2048)
/// - **2KB+ blocks**: Superblock at block 0 (offset 1024), GDT at block 1 (byte block_size)
///
/// # Arguments
/// * `sb` - The superblock (needed to know block size and group count)
/// * `volume` - The volume to read from
///
/// # Limitations
/// Each group descriptor is 32 bytes. Our 4KB buffer can hold 128 descriptors.
///
/// Currently returns an error if the GDT doesn't fit in our buffer.
pub fn read_group_descriptors(sb: &Ext2SuperBlock, volume: &dyn BlockVolume) -> Result<Vec<Ext2GroupDesc>, BlockError> {
    let num_groups = sb.num_groups();
    const MAX_GROUPS_IN_BUFFER: usize = IO_BUFFER_SIZE / GROUP_DESC_SIZE; // = 128

    // Check if GDT fits in our 4KB buffer
    if num_groups as usize > MAX_GROUPS_IN_BUFFER {
        kprintln!("ext2: ERROR - Filesystem has {} groups, but our 4KB buffer can only hold {} group descriptors",
                  num_groups, MAX_GROUPS_IN_BUFFER);
        return Err(BlockError::InvalidInput);
    }

    // Calculate GDT location based on block size
    // - 1KB blocks: superblock at block 1, GDT at block 2 (byte 2048)
    // - 2KB+ blocks: superblock at block 0, GDT at block 1 (byte block_size)
    let block_size = sb.block_size();
    let gdt_byte_offset = if block_size == MIN_BLOCK_SIZE {
        GDT_1K_OFFSET  // Block 2 for 1KB blocks
    } else {
        block_size as u64  // Block 1 for larger blocks
    };

    // Convert to sector number (512-byte sectors)
    let gdt_sector = gdt_byte_offset / BLOCK_SIZE as u64;

    kprintln!("ext2: Reading {} block group descriptor(s) from sector {} (byte {})",
              num_groups, gdt_sector, gdt_byte_offset);

    // Read GDT into buffer
    let buf = unsafe {
        let buf = &raw mut IO_BUFFER.0;
        let buf = &mut *buf;
        volume.read_blocks(gdt_sector, buf)?;
        buf as &[u8]
    };

    let mut groups = Vec::new();
    for i in 0..num_groups {
        let offset = i as usize * GROUP_DESC_SIZE;

        // Parse only the field we actually use
        let inode_table = buf.read_u32_le(offset + GD_INODE_TABLE_OFFSET);

        groups.push(Ext2GroupDesc {
            inode_table,
        });
    }

    Ok(groups)
}

/// ext2 inode structure (in-memory, parsed from disk)
///
/// Only includes the fields we actually use.
#[derive(Debug)]
pub struct Ext2Inode {
    pub mode: u16,          // File type and permissions (used in is_dir, is_reg)
    pub size: u32,          // File size in bytes (used in len, read_dir_entries)
    pub blocks: [u32; 15],  // Block pointers (used in read_dir_entries)
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
    // Inode numbers are 1-indexed
    if inode_num == 0 {
        return Err(BlockError::InvalidInput);
    }

    // Calculate which group contains this inode
    let group = ((inode_num - 1) / sb.inodes_per_group) as usize;
    let local_index = (inode_num - 1) % sb.inodes_per_group;

    if group >= groups.len() {
        return Err(BlockError::InvalidInput);
    }

    // Get inode table location from group descriptor
    let inode_table_block = groups[group].inode_table;

    // Get inode size from superblock (128 or 256 typically)
    let inode_size = sb.inode_size as u32;

    // Calculate which ext2 block contains this inode
    let block_size = sb.block_size();
    let inodes_per_block = block_size / inode_size;
    let block_offset = local_index / inodes_per_block;
    let inode_index_in_block = local_index % inodes_per_block;

    // Calculate the ext2 block number that contains this inode
    let target_block = inode_table_block + block_offset;

    // Convert to sector number (512-byte sectors)
    let sector = (target_block as u64 * block_size as u64) / BLOCK_SIZE as u64;

    // Read the full ext2 block containing the inode using static page-aligned buffer
    // (DMA requires 4KB-aligned buffers)
    let buf = unsafe {
        let buf = &raw mut IO_BUFFER.0;
        let buf = &mut *buf;
        volume.read_blocks(sector, buf)?;
        buf as &[u8]
    };

    // Calculate offset within the block
    let offset_in_block = (inode_index_in_block * inode_size) as usize;

    // Parse only the fields we actually use
    let mode = buf.read_u16_le(offset_in_block + INODE_MODE_OFFSET);
    let size = buf.read_u32_le(offset_in_block + INODE_SIZE_OFFSET);

    // Parse block pointers (15 u32 values at offset 40)
    let mut blocks = [0u32; 15];
    for i in 0..15 {
        blocks[i] = buf.read_u32_le(offset_in_block + INODE_BLOCKS_OFFSET + i * 4);
    }

    Ok(Ext2Inode {
        mode,
        size,
        blocks,
    })
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

    // Read entries from each direct block (we only support direct blocks for now)
    for block_ptr in inode.blocks.iter().take(NDIR_BLOCKS) {
        if *block_ptr == 0 {
            break; // No more blocks
        }

        if bytes_read >= inode.size {
            break; // Read all directory data
        }

        // Read the block using page-aligned static buffer
        let block_byte_offset = *block_ptr as u64 * block_size as u64;
        let sector = block_byte_offset / BLOCK_SIZE as u64;

        let block_buf = unsafe {
            let buf = &raw mut IO_BUFFER.0;
            let buf = &mut *buf;
            volume.read_blocks(sector, buf)?;
            buf as &[u8]
        };

        // Parse directory entries from this block
        let mut offset = 0;
        while offset < block_size as usize && bytes_read < inode.size {
            // Each entry is: inode(4) + rec_len(2) + name_len(1) + file_type(1) + name(variable)
            if offset + DIR_ENTRY_NAME_OFFSET > block_buf.len() {
                break;
            }

            let entry_inode = block_buf.read_u32_le(offset + DIR_ENTRY_INODE_OFFSET);
            let rec_len = block_buf.read_u16_le(offset + DIR_ENTRY_REC_LEN_OFFSET);
            let name_len = block_buf[offset + DIR_ENTRY_NAME_LEN_OFFSET];
            // file_type at DIR_ENTRY_FILE_TYPE_OFFSET (not used yet)

            // rec_len of 0 is invalid, stop processing
            if rec_len == 0 {
                break;
            }

            // If inode is non-zero, this is a valid entry
            if entry_inode != 0 && name_len > 0 {
                let name_start = offset + DIR_ENTRY_NAME_OFFSET;
                let name_end = name_start + name_len as usize;

                if name_end <= block_buf.len() {
                    if let Ok(name) = core::str::from_utf8(&block_buf[name_start..name_end]) {
                        entries.push((entry_inode, String::from(name)));
                    }
                }
            }

            // Move to next entry
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

    Err(BlockError::InvalidInput) // Not found
}

/// Test function to verify ext2 filesystem detection and structure
///
/// This function serves as an inspection vehicle for ext2 filesystem development.
/// It reads the superblock and group descriptors, displaying key information.
pub fn test_ext2_detect(volume: &dyn BlockVolume) {
    kprintln!("\nReading ext2 filesystem\n");

    // Step 1: Read superblock
    let sb = match read_superblock(volume) {
        Ok(sb) => sb,
        Err(e) => {
            kprintln!("ext2: Failed to read superblock: {:?}", e);
            return;
        }
    };

    // Superblock details are already printed by read_superblock()

    // Step 2: Read block group descriptors
    let groups = match read_group_descriptors(&sb, volume) {
        Ok(groups) => {
            for (i, group) in groups.iter().enumerate() {
                kprintln!("  Group {}: inode_table={}", i, group.inode_table);
            }
            groups
        }
        Err(e) => {
            kprintln!("ext2: Failed to read group descriptors: {:?}", e);
            return;
        }
    };

    // Step 3: Read root inode (inode #2 is always the root directory)
    let root_inode = match read_inode(volume, &sb, &groups, 2) {
        Ok(root_inode) => root_inode,
        Err(e) => {
            kprintln!("ext2: Failed to read root inode: {:?}", e);
            return;
        }
    };

    // Step 4: List root directory contents
    let entries = match read_dir_entries(volume, &sb, &root_inode) {
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

        if let Ok(found_ino) = lookup_entry(volume, &sb, &root_inode, target_name) {
            if found_ino == *target_ino {
                if let Ok(inode) = read_inode(volume, &sb, &groups, found_ino) {
                    let file_type = if inode.is_dir() { "directory" } else { "file" };
                    kprintln!("ext2: '{}' lookup: inode {}, {}, {} bytes",
                             target_name, found_ino, file_type, inode.len());
                }
            }
        }
    }
}
