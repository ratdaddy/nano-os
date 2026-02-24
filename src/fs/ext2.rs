//! ext2 filesystem implementation
//!
//! Read-only ext2 filesystem driver.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::str::from_utf8;

use crate::block::volume::BlockVolume;
use crate::bytes::ReadIntLe;
use crate::drivers::{BlockError, BLOCK_SIZE};
use crate::file::{Error, FileOps, FileType, Inode, InodeOps, SuperBlock};
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

// =============================================================================
// Ops tables
// =============================================================================

struct Ext2InodeOps;
struct Ext2FileOps;

static EXT2_INODE_OPS: Ext2InodeOps = Ext2InodeOps;
static EXT2_FILE_OPS: Ext2FileOps = Ext2FileOps;

impl InodeOps for Ext2InodeOps {
    fn lookup(&self, inode: &Arc<Inode>, name: &str) -> Result<Arc<Inode>, Error> {
        let data = inode.fs_data.downcast_ref::<Ext2InodeData>().unwrap();
        let sb = data.sb;
        let block_size = sb.block_size();

        if inode.file_type != FileType::Directory {
            return Err(Error::NotADirectory);
        }

        let dir_size = inode.len as u32;
        let dir_blocks = data.blocks;

        // Scan directory blocks for the named entry
        let mut bytes_read = 0u32;
        for block_ptr in dir_blocks.iter().take(NDIR_BLOCKS) {
            if *block_ptr == 0 || bytes_read >= dir_size {
                break;
            }

            let block_sector = (*block_ptr as u64 * block_size as u64) / BLOCK_SIZE as u64;

            // Scoped so buf is dropped before we call read_inode
            let found_ino = {
                let buf = unsafe {
                    let buf = &raw mut IO_BUFFER.0;
                    let buf = &mut *buf;
                    sb.volume.as_ref().read_blocks(block_sector, buf).map_err(|_| Error::NotFound)?;
                    buf as &[u8]
                };

                let mut found = None;
                let mut offset = 0;
                while offset < block_size as usize && bytes_read < dir_size {
                    if offset + DIR_ENTRY_NAME_OFFSET > buf.len() {
                        break;
                    }
                    let entry_inode = buf.read_u32_le(offset + DIR_ENTRY_INODE_OFFSET);
                    let rec_len = buf.read_u16_le(offset + DIR_ENTRY_REC_LEN_OFFSET);
                    let name_len = buf[offset + DIR_ENTRY_NAME_LEN_OFFSET];
                    if rec_len == 0 {
                        break;
                    }
                    if entry_inode != 0 && name_len > 0 {
                        let name_start = offset + DIR_ENTRY_NAME_OFFSET;
                        let name_end = name_start + name_len as usize;
                        if name_end <= buf.len() {
                            if let Ok(entry_name) = from_utf8(&buf[name_start..name_end]) {
                                if entry_name == name {
                                    found = Some(entry_inode);
                                }
                            }
                        }
                    }
                    offset += rec_len as usize;
                    bytes_read += rec_len as u32;
                }
                found
            };

            if let Some(ino) = found_ino {
                return sb.get_or_read_inode(ino).map_err(|_| Error::NotFound);
            }
        }

        Err(Error::NotFound)
    }
}
impl FileOps for Ext2FileOps {}

// =============================================================================
// Inode data
// =============================================================================

/// Filesystem-specific data stored in each ext2 `Inode::fs_data`.
struct Ext2InodeData {
    sb: &'static Ext2SuperBlock,
    blocks: [u32; 15],
}

// =============================================================================
// I/O buffer
// =============================================================================

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
const S_IFCHR: u16 = 0x2000;
#[allow(dead_code)]
const S_IFBLK: u16 = 0x6000;

// Special inode numbers
const ROOT_INODE: u32 = 2;

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
    root: Option<Arc<Inode>>,
    inode_cache: UnsafeCell<BTreeMap<u32, Weak<Inode>>>,
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
            inode_cache: UnsafeCell::new(BTreeMap::new()),
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

    /// Calculate number of block groups in the filesystem
    pub fn num_groups(&self) -> u32 {
        (self.blocks_count + self.blocks_per_group - 1) / self.blocks_per_group
    }

    /// Get the volume label
    pub fn volume_label(&self) -> Option<&str> {
        self.volume_label.as_deref()
    }

    /// Read an inode from disk and return a fully constructed VFS inode.
    ///
    /// Requires `&'static self` so the superblock reference can be stored in the inode's `fs_data`.
    fn read_inode(&'static self, inode_num: u32) -> Result<Arc<Inode>, BlockError> {
        let (sector, offset) = self.inode_location(inode_num)?;

        let buf = unsafe {
            let buf = &raw mut IO_BUFFER.0;
            let buf = &mut *buf;
            self.volume.as_ref().read_blocks(sector, buf)?;
            buf as &[u8]
        };

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
            _ => FileType::RegularFile,
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
    /// On a cache hit, upgrades the stored `Weak` and returns it without a disk read.
    /// On a miss, reads from disk and stores a `Weak` in the cache.
    fn get_or_read_inode(&'static self, inode_num: u32) -> Result<Arc<Inode>, BlockError> {
        let cache = unsafe { &mut *self.inode_cache.get() };
        if let Some(weak) = cache.get(&inode_num) {
            if let Some(arc) = weak.upgrade() {
                return Ok(arc);
            }
        }

        let inode = self.read_inode(inode_num)?;
        cache.retain(|_, weak| weak.upgrade().is_some());
        cache.insert(inode_num, Arc::downgrade(&inode));
        Ok(inode)
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

/// Parse directory entries from a directory inode's data blocks.
///
/// Returns a list of (inode_num, name) pairs for all entries in the directory.
///
/// # Arguments
/// * `volume` - The volume to read from
/// * `sb` - The superblock (for block size)
/// * `mode` - The inode mode field (must be a directory)
/// * `size` - The directory size in bytes
/// * `blocks` - The inode's direct block pointers
///
/// # Returns
/// Vector of (inode_number, filename) pairs
#[allow(dead_code)]
pub fn read_dir_entries(
    volume: &dyn BlockVolume,
    sb: &Ext2SuperBlock,
    mode: u16,
    size: u32,
    blocks: &[u32; 15],
) -> Result<Vec<(u32, String)>, BlockError> {

    if (mode & S_IFMT) != S_IFDIR {
        return Err(BlockError::InvalidInput);
    }

    let block_size = sb.block_size();
    let mut entries = Vec::new();
    let mut bytes_read = 0u32;

    // Only direct blocks supported for now
    for block_ptr in blocks.iter().take(NDIR_BLOCKS) {
        if *block_ptr == 0 || bytes_read >= size {
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
        while offset < block_size as usize && bytes_read < size {
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
                    if let Ok(name) = from_utf8(&block_buf[name_start..name_end]) {
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

/// Inspect an ext2 filesystem and display its structure
///
/// Reads the superblock, group descriptors, root directory, and tests lookup.
/// Used for development and debugging.
pub fn inspect_ext2(volume: Arc<dyn BlockVolume>) {
    kprintln!("\nReading ext2 filesystem\n");

    let sb = match Ext2SuperBlock::new(volume) {
        Ok(sb) => sb,
        Err(e) => { kprintln!("ext2: Failed to read ext2 filesystem: {:?}", e); return; }
    };

    // Print superblock info
    if let Some(label) = sb.volume_label() {
        kprintln!("ext2: {} blocks ({} bytes), {} inodes, {} groups ('{}')",
                 sb.blocks_count, sb.block_size(), sb.inodes_count, sb.num_groups(), label);
    } else {
        kprintln!("ext2: {} blocks ({} bytes), {} inodes, {} groups",
                 sb.blocks_count, sb.block_size(), sb.inodes_count, sb.num_groups());
    }
    for (i, group) in sb.groups.iter().enumerate() {
        kprintln!("  Group {}: inode_table={}", i, group.inode_table);
    }

    // Step 3: Display root inode via SuperBlock trait
    let root = sb.root_inode();
    kprintln!("ext2: root inode: ino={}, type={:?}, len={}", root.ino, root.file_type, root.len);

    // Step 4: List root directory contents (pending Ext2DirOps::readdir)

    // Step 5: Inode cache verification.
    // Lookups [1] and [2] held simultaneously: same address (cache hit).
    // Lookup [3] after both drop: new address (dead Weak upgraded fails, re-reads disk).
    // Lookup [4] of different inode: dead hello.txt entry reaped, count stays at 2.
    let cache_len = || unsafe { (*sb.inode_cache.get()).len() };

    kprintln!("ext2: cache count before lookups: {}", cache_len());
    {
        let inode1 = root.iops.lookup(&root, "hello.txt");
        let inode2 = root.iops.lookup(&root, "hello.txt");
        if let Ok(i) = &inode1 { kprintln!("ext2: lookup[1] hello.txt: addr={:#x}", Arc::as_ptr(i) as usize); }
        if let Ok(i) = &inode2 { kprintln!("ext2: lookup[2] hello.txt: addr={:#x}", Arc::as_ptr(i) as usize); }
        kprintln!("ext2: cache count while holding both: {}", cache_len());
    }
    kprintln!("ext2: cache count after drop: {}", cache_len());
    match root.iops.lookup(&root, "hello.txt") {
        Ok(i) => kprintln!("ext2: lookup[3] hello.txt: addr={:#x}", Arc::as_ptr(&i) as usize),
        Err(e) => kprintln!("ext2: lookup[3] hello.txt failed: {:?}", e),
    }
    kprintln!("ext2: cache count after lookup[3]: {}", cache_len());
    match root.iops.lookup(&root, "lost+found") {
        Ok(i) => kprintln!("ext2: lookup[4] lost+found: addr={:#x}", Arc::as_ptr(&i) as usize),
        Err(e) => kprintln!("ext2: lookup[4] lost+found failed: {:?}", e),
    }
    kprintln!("ext2: cache count after lookup[4]: {}", cache_len());
}

