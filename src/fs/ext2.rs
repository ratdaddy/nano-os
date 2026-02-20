//! ext2 filesystem implementation
//!
//! Read-only ext2 filesystem driver.

use alloc::vec::Vec;

use crate::block::volume::BlockVolume;
use crate::drivers::{BlockError, BLOCK_SIZE};

// ext2 superblock constants
const SUPERBLOCK_OFFSET: u64 = 1024;  // Superblock always starts at byte 1024
#[allow(dead_code)]
const SUPERBLOCK_SIZE: usize = 1024;  // Superblock is 1024 bytes
const SUPERBLOCK_SECTOR: u64 = SUPERBLOCK_OFFSET / BLOCK_SIZE as u64;  // Sector 2 for 512-byte blocks
const EXT2_MAGIC: u16 = 0xEF53;
const EXT2_MAGIC_OFFSET: usize = 56;
const EXT2_VOLUME_LABEL_OFFSET: usize = 120;
const EXT2_VOLUME_LABEL_LEN: usize = 16;

/// ext2 superblock structure (simplified)
///
/// Only includes the fields we need for basic read operations.
#[repr(C)]
#[derive(Debug)]
pub struct Ext2Superblock {
    pub s_inodes_count: u32,
    pub s_blocks_count: u32,
    pub s_r_blocks_count: u32,
    pub s_free_blocks_count: u32,
    pub s_free_inodes_count: u32,
    pub s_first_data_block: u32,
    pub s_log_block_size: u32,
    pub s_log_frag_size: u32,
    pub s_blocks_per_group: u32,
    pub s_frags_per_group: u32,
    pub s_inodes_per_group: u32,
    pub s_mtime: u32,
    pub s_wtime: u32,
    pub s_mnt_count: u16,
    pub s_max_mnt_count: u16,
    pub s_magic: u16,
    // ... remaining fields omitted for now
}

impl Ext2Superblock {
    /// Calculate the actual block size from the log value
    pub fn block_size(&self) -> u32 {
        1024 << self.s_log_block_size
    }

    /// Calculate number of block groups in the filesystem
    pub fn num_groups(&self) -> u32 {
        (self.s_blocks_count + self.s_blocks_per_group - 1) / self.s_blocks_per_group
    }
}

/// ext2 block group descriptor structure
///
/// Each block group has a descriptor that locates the block bitmap,
/// inode bitmap, and inode table for that group.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Ext2GroupDesc {
    pub bg_block_bitmap: u32,      // Block number of block bitmap
    pub bg_inode_bitmap: u32,      // Block number of inode bitmap
    pub bg_inode_table: u32,       // Block number of first inode table block
    pub bg_free_blocks_count: u16, // Free blocks in group
    pub bg_free_inodes_count: u16, // Free inodes in group
    pub bg_used_dirs_count: u16,   // Number of directories in group
    pub bg_pad: u16,               // Padding
    pub bg_reserved: [u32; 3],     // Reserved for future use
}

// Static buffer for ext2 block I/O (4KB) - must be page-aligned for DMA
// Page alignment (4096) ensures buffer doesn't cross physical page boundaries
#[repr(C, align(4096))]
struct Ext2BlockBuffer([u8; 4096]);

static mut SUPERBLOCK_BUFFER: Ext2BlockBuffer = Ext2BlockBuffer([0; 4096]);

/// Read and parse the ext2 superblock from a volume
///
/// The ext2 superblock is always 1024 bytes starting at byte offset 1024.
/// With 512-byte blocks, this means sectors 2-3 contain the superblock.
/// We read 8 sectors (4096 bytes) starting at sector 2, which includes the
/// full superblock plus additional data we can ignore.
pub fn read_superblock(volume: &dyn BlockVolume) -> Result<Ext2Superblock, BlockError> {
    // Verify our assumption about block size
    assert_eq!(volume.block_size(), BLOCK_SIZE as u32, "Volume block size mismatch");

    // Read 8 sectors (4096 bytes) starting at sector 2
    // This gives us the full superblock (1024 bytes at start of buffer)
    let buf = unsafe {
        let buf = &raw mut SUPERBLOCK_BUFFER.0;
        let buf = &mut *buf;
        volume.read_blocks(SUPERBLOCK_SECTOR, buf)?;
        buf
    };

    // Check magic number first
    let magic = u16::from_le_bytes(
        buf[EXT2_MAGIC_OFFSET..EXT2_MAGIC_OFFSET + 2].try_into().unwrap()
    );

    if magic != EXT2_MAGIC {
        kprintln!("ext2: Invalid magic number {:#x} (expected {:#x})", magic, EXT2_MAGIC);
        return Err(BlockError::InvalidInput);
    }

    // Parse superblock fields (all little-endian)
    let s_inodes_count = u32::from_le_bytes(buf[0..4].try_into().unwrap());
    let s_blocks_count = u32::from_le_bytes(buf[4..8].try_into().unwrap());
    let s_r_blocks_count = u32::from_le_bytes(buf[8..12].try_into().unwrap());
    let s_free_blocks_count = u32::from_le_bytes(buf[12..16].try_into().unwrap());
    let s_free_inodes_count = u32::from_le_bytes(buf[16..20].try_into().unwrap());
    let s_first_data_block = u32::from_le_bytes(buf[20..24].try_into().unwrap());
    let s_log_block_size = u32::from_le_bytes(buf[24..28].try_into().unwrap());
    let s_log_frag_size = u32::from_le_bytes(buf[28..32].try_into().unwrap());
    let s_blocks_per_group = u32::from_le_bytes(buf[32..36].try_into().unwrap());
    let s_frags_per_group = u32::from_le_bytes(buf[36..40].try_into().unwrap());
    let s_inodes_per_group = u32::from_le_bytes(buf[40..44].try_into().unwrap());
    let s_mtime = u32::from_le_bytes(buf[44..48].try_into().unwrap());
    let s_wtime = u32::from_le_bytes(buf[48..52].try_into().unwrap());
    let s_mnt_count = u16::from_le_bytes(buf[52..54].try_into().unwrap());
    let s_max_mnt_count = u16::from_le_bytes(buf[54..56].try_into().unwrap());

    let sb = Ext2Superblock {
        s_inodes_count,
        s_blocks_count,
        s_r_blocks_count,
        s_free_blocks_count,
        s_free_inodes_count,
        s_first_data_block,
        s_log_block_size,
        s_log_frag_size,
        s_blocks_per_group,
        s_frags_per_group,
        s_inodes_per_group,
        s_mtime,
        s_wtime,
        s_mnt_count,
        s_max_mnt_count,
        s_magic: magic,
    };

    kprintln!("ext2: Superblock parsed successfully");
    kprintln!("  Inodes: {}", sb.s_inodes_count);
    kprintln!("  Blocks: {}", sb.s_blocks_count);
    kprintln!("  Block size: {} bytes", sb.block_size());
    kprintln!("  Inodes per group: {}", sb.s_inodes_per_group);
    kprintln!("  Blocks per group: {}", sb.s_blocks_per_group);

    // Read volume label
    let label_bytes = &buf[EXT2_VOLUME_LABEL_OFFSET..EXT2_VOLUME_LABEL_OFFSET + EXT2_VOLUME_LABEL_LEN];
    if let Some(null_pos) = label_bytes.iter().position(|&b| b == 0) {
        if null_pos > 0 {
            if let Ok(label) = core::str::from_utf8(&label_bytes[..null_pos]) {
                kprintln!("  Volume label: '{}'", label);
            }
        }
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
pub fn read_group_descriptors(sb: &Ext2Superblock, volume: &dyn BlockVolume) -> Result<Vec<Ext2GroupDesc>, BlockError> {
    let num_groups = sb.num_groups();
    const GROUP_DESC_SIZE: usize = 32;
    const MAX_GROUPS_IN_BUFFER: usize = 4096 / GROUP_DESC_SIZE; // = 128

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
    let gdt_byte_offset = if block_size == 1024 {
        2048  // Block 2 for 1KB blocks
    } else {
        block_size as u64  // Block 1 for larger blocks
    };

    // Convert to sector number (512-byte sectors)
    let gdt_sector = gdt_byte_offset / BLOCK_SIZE as u64;

    kprintln!("ext2: Reading {} block group descriptor(s) from sector {} (byte {})",
              num_groups, gdt_sector, gdt_byte_offset);

    // Read GDT into buffer
    let buf = unsafe {
        let buf = &raw mut SUPERBLOCK_BUFFER.0;
        let buf = &mut *buf;
        volume.read_blocks(gdt_sector, buf)?;
        buf as &[u8]
    };

    let mut groups = Vec::new();
    for i in 0..num_groups {
        let offset = i as usize * GROUP_DESC_SIZE;

        // Parse group descriptor fields (all little-endian)
        // (Buffer overflow checked above - we know all groups fit)
        let bg_block_bitmap = u32::from_le_bytes(buf[offset..offset+4].try_into().unwrap());
        let bg_inode_bitmap = u32::from_le_bytes(buf[offset+4..offset+8].try_into().unwrap());
        let bg_inode_table = u32::from_le_bytes(buf[offset+8..offset+12].try_into().unwrap());
        let bg_free_blocks_count = u16::from_le_bytes(buf[offset+12..offset+14].try_into().unwrap());
        let bg_free_inodes_count = u16::from_le_bytes(buf[offset+14..offset+16].try_into().unwrap());
        let bg_used_dirs_count = u16::from_le_bytes(buf[offset+16..offset+18].try_into().unwrap());
        let bg_pad = u16::from_le_bytes(buf[offset+18..offset+20].try_into().unwrap());
        let bg_reserved = [
            u32::from_le_bytes(buf[offset+20..offset+24].try_into().unwrap()),
            u32::from_le_bytes(buf[offset+24..offset+28].try_into().unwrap()),
            u32::from_le_bytes(buf[offset+28..offset+32].try_into().unwrap()),
        ];

        groups.push(Ext2GroupDesc {
            bg_block_bitmap,
            bg_inode_bitmap,
            bg_inode_table,
            bg_free_blocks_count,
            bg_free_inodes_count,
            bg_used_dirs_count,
            bg_pad,
            bg_reserved,
        });
    }

    Ok(groups)
}

/// Test function to verify ext2 filesystem detection and structure
///
/// This function serves as an inspection vehicle for ext2 filesystem development.
/// It reads the superblock and group descriptors, displaying key information.
pub fn test_ext2_detect(volume: &dyn BlockVolume) {
    kprintln!("\n=== ext2 Filesystem Inspection ===");

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
    match read_group_descriptors(&sb, volume) {
        Ok(groups) => {
            kprintln!("\next2: Block Group Descriptors:");
            for (i, group) in groups.iter().enumerate() {
                kprintln!("  Group {}:", i);
                kprintln!("    Block bitmap at block: {}", group.bg_block_bitmap);
                kprintln!("    Inode bitmap at block: {}", group.bg_inode_bitmap);
                kprintln!("    Inode table at block:  {}", group.bg_inode_table);
                kprintln!("    Free blocks: {}", group.bg_free_blocks_count);
                kprintln!("    Free inodes: {}", group.bg_free_inodes_count);
                kprintln!("    Directories: {}", group.bg_used_dirs_count);
            }
        }
        Err(e) => {
            kprintln!("\next2: Failed to read group descriptors: {:?}", e);
        }
    }
}
