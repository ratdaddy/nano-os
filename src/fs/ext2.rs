//! ext2 filesystem implementation
//!
//! Read-only ext2 filesystem driver.

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
}

// Static buffer for superblock I/O - must be properly aligned for DMA
#[repr(C, align(512))]
struct SectorBuffer([u8; BLOCK_SIZE]);

static mut SUPERBLOCK_BUFFER: SectorBuffer = SectorBuffer([0; BLOCK_SIZE]);

/// Read and parse the ext2 superblock from a volume
///
/// The ext2 superblock is always 1024 bytes starting at byte offset 1024.
/// With 512-byte blocks, this means reading sector 2 (and sector 3 for the full superblock).
/// For now, we only read sector 2 since all the fields we need are in the first 512 bytes.
pub fn read_superblock(volume: &dyn BlockVolume) -> Result<Ext2Superblock, BlockError> {
    // Verify our assumption about block size
    assert_eq!(volume.block_size(), BLOCK_SIZE as u32, "Volume block size mismatch");

    // Read sector 2 which contains the start of the superblock (bytes 1024-1535)
    // The superblock fields we need (magic, volume label) are all within the first 512 bytes
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

/// Test function to verify ext2 filesystem detection
pub fn test_ext2_detect(volume: &dyn BlockVolume) {
    kprintln!("\n=== ext2 Detection Test ===");

    match read_superblock(volume) {
        Ok(sb) => {
            kprintln!("ext2: Superblock read successfully");
            kprintln!("  Magic: {:#x}", sb.s_magic);
            kprintln!("  Blocks: {}", sb.s_blocks_count);
            kprintln!("  Inodes: {}", sb.s_inodes_count);
            kprintln!("  Block size: {}", sb.block_size());
        }
        Err(e) => {
            kprintln!("ext2: Failed to read superblock: {:?}", e);
        }
    }
}
