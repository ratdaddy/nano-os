//! ext2 filesystem implementation
//!
//! Read-only ext2 filesystem driver.

use alloc::string::String;
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

/// ext2 superblock structure (in-memory, parsed from disk)
///
/// Only includes the fields we need for basic read operations.
#[derive(Debug)]
pub struct Ext2SuperBlock {
    pub s_inodes_count: u32,
    pub s_blocks_count: u32,
    #[allow(dead_code)]
    pub s_r_blocks_count: u32,
    #[allow(dead_code)]
    pub s_free_blocks_count: u32,
    #[allow(dead_code)]
    pub s_free_inodes_count: u32,
    #[allow(dead_code)]
    pub s_first_data_block: u32,
    pub s_log_block_size: u32,
    #[allow(dead_code)]
    pub s_log_frag_size: u32,
    pub s_blocks_per_group: u32,
    #[allow(dead_code)]
    pub s_frags_per_group: u32,
    pub s_inodes_per_group: u32,
    #[allow(dead_code)]
    pub s_mtime: u32,
    #[allow(dead_code)]
    pub s_wtime: u32,
    #[allow(dead_code)]
    pub s_mnt_count: u16,
    #[allow(dead_code)]
    pub s_max_mnt_count: u16,
    #[allow(dead_code)]
    pub s_magic: u16,
    pub s_inode_size: u16,  // Size of inode structure (128 or 256 typically)
    // ... remaining fields omitted for now
}

impl Ext2SuperBlock {
    /// Calculate the actual block size from the log value
    pub fn block_size(&self) -> u32 {
        1024 << self.s_log_block_size
    }

    /// Calculate number of block groups in the filesystem
    pub fn num_groups(&self) -> u32 {
        (self.s_blocks_count + self.s_blocks_per_group - 1) / self.s_blocks_per_group
    }
}

/// ext2 block group descriptor structure (in-memory, parsed from disk)
///
/// Each block group has a descriptor that locates the block bitmap,
/// inode bitmap, and inode table for that group.
#[derive(Debug, Clone, Copy)]
pub struct Ext2GroupDesc {
    pub bg_block_bitmap: u32,      // Block number of block bitmap
    pub bg_inode_bitmap: u32,      // Block number of inode bitmap
    pub bg_inode_table: u32,       // Block number of first inode table block
    pub bg_free_blocks_count: u16, // Free blocks in group
    pub bg_free_inodes_count: u16, // Free inodes in group
    pub bg_used_dirs_count: u16,   // Number of directories in group
    #[allow(dead_code)]
    pub bg_pad: u16,               // Padding
    #[allow(dead_code)]
    pub bg_reserved: [u32; 3],     // Reserved for future use
}

// Static buffer for ext2 block I/O (4KB) - must be page-aligned for DMA
// Page alignment (4096) ensures buffer doesn't cross physical page boundaries
// Used for reading superblock, group descriptors, inodes, and directory entries
#[repr(C, align(4096))]
struct Ext2BlockBuffer([u8; 4096]);

static mut EXT2_IO_BUFFER: Ext2BlockBuffer = Ext2BlockBuffer([0; 4096]);

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
        let buf = &raw mut EXT2_IO_BUFFER.0;
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
    // s_inode_size is at offset 88 (only valid for revision >= 1)
    let s_inode_size = u16::from_le_bytes(buf[88..90].try_into().unwrap());
    // Default to 128 if zero (revision 0 always uses 128-byte inodes)
    let s_inode_size = if s_inode_size == 0 { 128 } else { s_inode_size };

    let sb = Ext2SuperBlock {
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
        s_inode_size,
    };

    kprintln!("ext2: Superblock parsed successfully");
    kprintln!("  Inodes: {}", sb.s_inodes_count);
    kprintln!("  Blocks: {}", sb.s_blocks_count);
    kprintln!("  Block size: {} bytes", sb.block_size());
    kprintln!("  Inode size: {} bytes", sb.s_inode_size);
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
pub fn read_group_descriptors(sb: &Ext2SuperBlock, volume: &dyn BlockVolume) -> Result<Vec<Ext2GroupDesc>, BlockError> {
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
        let buf = &raw mut EXT2_IO_BUFFER.0;
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

/// ext2 inode structure (in-memory, parsed from disk)
///
/// Standard ext2 inode is 128 bytes. Contains file metadata and block pointers.
#[derive(Debug)]
pub struct Ext2Inode {
    pub i_mode: u16,           // File type and permissions
    pub i_size: u32,           // File size in bytes
    pub i_block: [u32; 15],    // Block pointers (12 direct + 3 indirect)
    #[allow(dead_code)]
    pub i_uid: u16,            // User ID
    #[allow(dead_code)]
    pub i_gid: u16,            // Group ID
    #[allow(dead_code)]
    pub i_links_count: u16,    // Hard link count
    #[allow(dead_code)]
    pub i_blocks: u32,         // Number of 512-byte blocks allocated
}

impl Ext2Inode {
    /// Get file size in bytes
    pub fn len(&self) -> usize {
        self.i_size as usize
    }

    /// Check if this inode represents a directory
    pub fn is_dir(&self) -> bool {
        const S_IFDIR: u16 = 0x4000;
        (self.i_mode & 0xF000) == S_IFDIR
    }

    /// Check if this inode represents a regular file
    #[allow(dead_code)]
    pub fn is_reg(&self) -> bool {
        const S_IFREG: u16 = 0x8000;
        (self.i_mode & 0xF000) == S_IFREG
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
    let group = ((inode_num - 1) / sb.s_inodes_per_group) as usize;
    let local_index = (inode_num - 1) % sb.s_inodes_per_group;

    if group >= groups.len() {
        return Err(BlockError::InvalidInput);
    }

    // Get inode table location from group descriptor
    let inode_table_block = groups[group].bg_inode_table;

    // Get inode size from superblock (128 or 256 typically)
    let inode_size = sb.s_inode_size as u32;

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
        let buf = &raw mut EXT2_IO_BUFFER.0;
        let buf = &mut *buf;
        volume.read_blocks(sector, buf)?;
        buf as &[u8]
    };

    // Calculate offset within the block
    let offset_in_block = (inode_index_in_block * inode_size) as usize;

    // Parse inode fields (all little-endian)
    let i_mode = u16::from_le_bytes(buf[offset_in_block..offset_in_block+2].try_into().unwrap());
    let i_uid = u16::from_le_bytes(buf[offset_in_block+2..offset_in_block+4].try_into().unwrap());
    let i_size = u32::from_le_bytes(buf[offset_in_block+4..offset_in_block+8].try_into().unwrap());
    // Skip atime, ctime, mtime, dtime (offsets 8-24)
    let i_gid = u16::from_le_bytes(buf[offset_in_block+24..offset_in_block+26].try_into().unwrap());
    let i_links_count = u16::from_le_bytes(buf[offset_in_block+26..offset_in_block+28].try_into().unwrap());
    let i_blocks = u32::from_le_bytes(buf[offset_in_block+28..offset_in_block+32].try_into().unwrap());
    // Skip flags and osd1 (offsets 32-40)

    // Parse block pointers (15 u32 values at offset 40)
    let mut i_block = [0u32; 15];
    for i in 0..15 {
        let offset = offset_in_block + 40 + i * 4;
        i_block[i] = u32::from_le_bytes(buf[offset..offset+4].try_into().unwrap());
    }

    Ok(Ext2Inode {
        i_mode,
        i_size,
        i_block,
        i_uid,
        i_gid,
        i_links_count,
        i_blocks,
    })
}

/// Parse directory entries from a directory inode
///
/// Returns a list of (inode_num, name) pairs for all entries in the directory.
/// Directory entries are stored in the data blocks pointed to by the inode's i_block array.
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
    for block_ptr in inode.i_block.iter().take(12) {
        if *block_ptr == 0 {
            break; // No more blocks
        }

        if bytes_read >= inode.i_size {
            break; // Read all directory data
        }

        // Read the block using page-aligned static buffer
        let block_byte_offset = *block_ptr as u64 * block_size as u64;
        let sector = block_byte_offset / BLOCK_SIZE as u64;

        let block_buf = unsafe {
            let buf = &raw mut EXT2_IO_BUFFER.0;
            let buf = &mut *buf;
            volume.read_blocks(sector, buf)?;
            buf as &[u8]
        };

        // Parse directory entries from this block
        let mut offset = 0;
        while offset < block_size as usize && bytes_read < inode.i_size {
            // Each entry is: inode(4) + rec_len(2) + name_len(1) + file_type(1) + name(variable)
            if offset + 8 > block_buf.len() {
                break;
            }

            let entry_inode = u32::from_le_bytes(block_buf[offset..offset+4].try_into().unwrap());
            let rec_len = u16::from_le_bytes(block_buf[offset+4..offset+6].try_into().unwrap());
            let name_len = block_buf[offset + 6];
            // file_type at offset+7 (not used yet)

            // rec_len of 0 is invalid, stop processing
            if rec_len == 0 {
                break;
            }

            // If inode is non-zero, this is a valid entry
            if entry_inode != 0 && name_len > 0 {
                let name_start = offset + 8;
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
    let groups = match read_group_descriptors(&sb, volume) {
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
            groups
        }
        Err(e) => {
            kprintln!("\next2: Failed to read group descriptors: {:?}", e);
            return;
        }
    };

    // Step 3: Read root inode (inode #2 is always the root directory)
    let root_inode = match read_inode(volume, &sb, &groups, 2) {
        Ok(root_inode) => {
            kprintln!("\next2: Root Inode (inode #2):");
            kprintln!("  Mode: {:#06x} ({})", root_inode.i_mode,
                     if root_inode.is_dir() { "directory" } else { "file" });
            kprintln!("  Size: {} bytes", root_inode.len());
            kprintln!("  Links: {}", root_inode.i_links_count);
            kprintln!("  Blocks: {}", root_inode.i_blocks);
            kprintln!("  First block pointer: {}", root_inode.i_block[0]);
            root_inode
        }
        Err(e) => {
            kprintln!("\next2: Failed to read root inode: {:?}", e);
            return;
        }
    };

    // Step 4: List root directory contents
    let entries = match read_dir_entries(volume, &sb, &root_inode) {
        Ok(entries) => {
            kprintln!("\next2: Root Directory Contents:");
            for (inode_num, name) in &entries {
                kprintln!("  {} (inode {})", name, inode_num);
            }
            kprintln!("  Total entries: {}", entries.len());
            entries
        }
        Err(e) => {
            kprintln!("\next2: Failed to read directory entries: {:?}", e);
            return;
        }
    };

    // Step 5: Test lookup functionality
    kprintln!("\next2: VFS Function Verification:");
    kprintln!("  ✓ read_superblock() - parsed superblock");
    kprintln!("  ✓ read_group_descriptors() - read {} groups", groups.len());
    kprintln!("  ✓ read_inode() - read root inode #2");
    kprintln!("  ✓ read_dir_entries() - listed {} entries", entries.len());

    // Try to lookup a known entry (skip "." and "..")
    if let Some((target_ino, target_name)) = entries.iter()
        .find(|(_, name)| name != "." && name != "..") {

        kprintln!("\next2: Testing lookup_entry():");
        kprintln!("  Looking up '{}'...", target_name);

        match lookup_entry(volume, &sb, &root_inode, target_name) {
            Ok(found_ino) => {
                if found_ino == *target_ino {
                    kprintln!("  ✓ lookup_entry() - found inode {} (correct!)", found_ino);

                    // Read the looked-up inode to verify
                    match read_inode(volume, &sb, &groups, found_ino) {
                        Ok(inode) => {
                            kprintln!("  ✓ Successfully read inode:");
                            kprintln!("      Type: {}", if inode.is_dir() { "directory" } else { "file" });
                            kprintln!("      Size: {} bytes", inode.len());
                        }
                        Err(e) => {
                            kprintln!("  ✗ Failed to read inode: {:?}", e);
                        }
                    }
                } else {
                    kprintln!("  ✗ lookup_entry() returned wrong inode {} (expected {})",
                             found_ino, target_ino);
                }
            }
            Err(e) => {
                kprintln!("  ✗ lookup_entry() failed: {:?}", e);
            }
        }
    }

    kprintln!("\n=== ext2 Inspection Complete ===");
}
