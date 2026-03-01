//! ext2 filesystem implementation
//!
//! Read-only ext2 filesystem driver.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::str::from_utf8;

use crate::block::volume::{BlockBuf, BlockVolume, CACHE_BLOCK_SIZE};
use crate::bytes::ReadIntLe;
use crate::collections::LruCache;
use crate::drivers::{BlockError, BLOCK_SIZE};
use crate::file::{DirEntry, Error, File, FileOps, FileType, Inode, InodeOps, SeekFrom, SuperBlock};
use crate::file::{S_IFMT, S_IFREG, S_IFDIR, S_IFCHR, S_IFBLK};
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
        if inode.file_type != FileType::Directory {
            return Err(Error::NotADirectory);
        }

        let fs_data = inode.fs_data.downcast_ref::<Ext2InodeData>().unwrap();

        for entry in DirEntryIter::new(inode) {
            let (ino, entry_name, _) = entry.map_err(|_| Error::NotFound)?;
            if entry_name == name {
                return fs_data.sb.get_or_read_inode(ino).map_err(|_| Error::NotFound);
            }
        }

        Err(Error::NotFound)
    }
}
impl FileOps for Ext2FileOps {
    fn read(&self, file: &mut File, buf: &mut [u8]) -> Result<usize, Error> {
        if file.inode.file_type != FileType::RegularFile {
            return Err(Error::InvalidInput);
        }

        let file_len = file.inode.len;
        if file.offset >= file_len || buf.is_empty() {
            return Ok(0);
        }

        let fs_data = file.inode.fs_data.downcast_ref::<Ext2InodeData>().unwrap();
        let sb = fs_data.sb;
        let block_size = sb.block_size() as usize;
        let mut buf_pos = 0;

        while buf_pos < buf.len() && file.offset < file_len {
            let block_idx = file.offset / block_size;
            if block_idx >= NDIR_BLOCKS {
                break; // indirect blocks not yet supported
            }
            let block_ptr = fs_data.blocks[block_idx];
            if block_ptr == 0 {
                break; // sparse hole or end of allocated blocks
            }

            let block_buf = sb.volume.as_ref()
                .get_block(sb.block_to_sector(block_ptr))
                .map_err(|_| Error::InvalidInput)?;

            let block_offset  = file.offset % block_size;
            let copy_len = (block_size - block_offset)
                .min(file_len - file.offset)
                .min(buf.len() - buf_pos);

            buf[buf_pos..buf_pos + copy_len]
                .copy_from_slice(&block_buf[block_offset..block_offset + copy_len]);

            file.offset += copy_len;
            buf_pos     += copy_len;
            // block_buf dropped here — cache slot unpinned
        }

        Ok(buf_pos)
    }

    fn readdir(&self, file: &mut File) -> Result<Vec<DirEntry>, Error> {
        if file.inode.file_type != FileType::Directory {
            return Err(Error::NotADirectory);
        }

        DirEntryIter::new(&file.inode)
            .map(|r| {
                let (_, name, ft_byte) = r.map_err(|_| Error::InvalidInput)?;
                let file_type = match ft_byte {
                    EXT2_FT_REG_FILE => FileType::RegularFile,
                    EXT2_FT_DIR      => FileType::Directory,
                    EXT2_FT_CHRDEV   => FileType::CharDevice,
                    EXT2_FT_BLKDEV   => FileType::BlockDevice,
                    _                => FileType::RegularFile,
                };
                Ok(DirEntry { name, file_type })
            })
            .collect()
    }
}

// =============================================================================
// Inode data
// =============================================================================

/// Filesystem-specific data stored in each ext2 `Inode::fs_data`.
struct Ext2InodeData {
    sb: &'static Ext2SuperBlock,
    blocks: [u32; 15],
}

// =============================================================================
// Inode cache
// =============================================================================

const INODE_CACHE_CAPACITY: usize = 32;

// =============================================================================
// Directory iterator
// =============================================================================

/// An iterator over raw directory entries in an ext2 directory inode.
///
/// Reads direct data blocks one at a time, yielding `(inode_num, name, file_type_byte)`
/// for each valid entry. Deleted entries (inode_num == 0) are skipped automatically.
/// Disk I/O errors are surfaced as `Err(BlockError)` rather than panicking.
///
/// Construction is infallible; the first block is loaded lazily on the initial `next()` call.
pub struct DirEntryIter {
    sb: &'static Ext2SuperBlock,
    blocks: [u32; 15],
    dir_size: u32,
    buf: Option<Arc<BlockBuf>>,
    block_size: usize,
    block_idx: usize,    // index into blocks[] of the currently loaded (or next to load) block
    block_offset: usize, // byte offset within buf; initialized to block_size as a "not loaded" sentinel
    bytes_read: u32,
}

impl DirEntryIter {
    /// Create an iterator over the directory entries of `inode`.
    ///
    /// `inode` must be a directory backed by `Ext2InodeData`; panics otherwise.
    pub fn new(inode: &Inode) -> Self {
        let fs_data = inode.fs_data.downcast_ref::<Ext2InodeData>().unwrap();
        let block_size = fs_data.sb.block_size() as usize;
        Self {
            sb: fs_data.sb,
            blocks: fs_data.blocks,
            dir_size: inode.len as u32,
            buf: None,
            block_size,
            block_idx: 0,
            block_offset: block_size, // sentinel: triggers first block load on next()
            bytes_read: 0,
        }
    }
}

impl Iterator for DirEntryIter {
    type Item = Result<(u32, String, u8), BlockError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // Load the next block when the current one is exhausted (or on first call).
            if self.block_offset >= self.block_size {
                if self.block_idx >= NDIR_BLOCKS || self.bytes_read >= self.dir_size {
                    return None;
                }
                let block_ptr = self.blocks[self.block_idx];
                if block_ptr == 0 {
                    return None;
                }
                let sector = self.sb.block_to_sector(block_ptr);
                match self.sb.volume.as_ref().get_block(sector) {
                    Ok(block) => self.buf = Some(block),
                    Err(e)    => return Some(Err(e)),
                }
                self.block_offset = 0;
            }

            let buf    = self.buf.as_deref().unwrap();
            let offset = self.block_offset;

            if offset + DIR_ENTRY_NAME_OFFSET > self.block_size {
                // Misaligned — block is likely corrupt; skip to the next one.
                self.block_idx   += 1;
                self.block_offset = self.block_size;
                continue;
            }

            let ino      = buf.read_u32_le(offset + DIR_ENTRY_INODE_OFFSET);
            let rec_len  = buf.read_u16_le(offset + DIR_ENTRY_REC_LEN_OFFSET) as usize;
            let name_len = buf[offset + DIR_ENTRY_NAME_LEN_OFFSET] as usize;
            let ft_byte  = buf[offset + DIR_ENTRY_FILE_TYPE_OFFSET];

            if rec_len == 0 {
                // rec_len == 0 would loop forever; treat as end of block.
                self.block_idx   += 1;
                self.block_offset = self.block_size;
                continue;
            }

            self.block_offset += rec_len;
            self.bytes_read   += rec_len as u32;

            // If we've reached either a block boundary or the end of the directory
            // data, schedule the next block load (or signal exhaustion at the top).
            if self.block_offset >= self.block_size || self.bytes_read >= self.dir_size {
                self.block_idx   += 1;
                self.block_offset = self.block_size;
            }

            if ino == 0 || name_len == 0 {
                continue; // deleted or empty entry
            }

            let name_start = offset + DIR_ENTRY_NAME_OFFSET;
            let name_end   = name_start + name_len;
            if name_end > self.block_size {
                continue; // corrupt name length
            }

            match from_utf8(&buf[name_start..name_end]) {
                Ok(name) => return Some(Ok((ino, String::from(name), ft_byte))),
                Err(_)   => continue, // non-UTF-8 name; skip
            }
        }
    }
}

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

// Block and inode constants
const MIN_BLOCK_SIZE: u32 = 1024;
const GOOD_OLD_INODE_SIZE: u16 = 128;
const NDIR_BLOCKS: usize = 12;

// Group descriptor constants
const GROUP_DESC_SIZE: usize = 32;
const GDT_1K_OFFSET: u64 = 2048;
const GD_INODE_TABLE_OFFSET: usize = 8;

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
const DIR_ENTRY_FILE_TYPE_OFFSET: usize = 7;
const DIR_ENTRY_NAME_OFFSET: usize = 8;

// Directory entry file type values
const EXT2_FT_REG_FILE: u8 = 1;
const EXT2_FT_DIR: u8 = 2;
const EXT2_FT_CHRDEV: u8 = 3;
const EXT2_FT_BLKDEV: u8 = 4;

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
    inode_cache: UnsafeCell<LruCache<u32, Inode>>,
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
    fn block_to_sector(&self, block_num: u32) -> u64 {
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

    /// Read an inode from disk and return a fully constructed VFS inode.
    ///
    /// Requires `&'static self` so the superblock reference can be stored in the inode's `fs_data`.
    fn read_inode(&'static self, inode_num: u32) -> Result<Arc<Inode>, BlockError> {
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
    /// On a cache hit, moves the entry to the front of the LRU and returns it
    /// without a disk read. On a miss, reads from disk and inserts into the cache.
    fn get_or_read_inode(&'static self, inode_num: u32) -> Result<Arc<Inode>, BlockError> {
        let cache = unsafe { &mut *self.inode_cache.get() };
        if let Some(arc) = cache.get(&inode_num) {
            return Ok(arc);
        }
        let inode = self.read_inode(inode_num)?;
        cache.insert(inode_num, Arc::clone(&inode));
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

    // Step 4: List root directory contents via readdir
    let mut root_file = root.fops.open(Arc::clone(&root)).unwrap();
    match root_file.fops.readdir(&mut root_file) {
        Ok(entries) => {
            kprintln!("ext2: root directory ({} entries):", entries.len());
            for entry in &entries {
                let type_char = match entry.file_type {
                    FileType::Directory   => 'd',
                    FileType::RegularFile => 'f',
                    FileType::CharDevice  => 'c',
                    FileType::BlockDevice => 'b',
                };
                kprintln!("  {} {}", type_char, entry.name);
            }
        }
        Err(e) => kprintln!("ext2: readdir failed: {:?}", e),
    }

    // Step 5: Read hello.txt — seek + multi-part read
    match root.iops.lookup(&root, "hello.txt") {
        Ok(inode) => {
            let mut file = inode.fops.open(Arc::clone(&inode)).unwrap();
            let fops = file.fops;

            // Full read from offset 0.
            let mut buf_full = [0u8; 256];
            match fops.read(&mut file, &mut buf_full) {
                Ok(n) => kprintln!("ext2: hello.txt full: {:?}", core::str::from_utf8(&buf_full[..n]).unwrap_or("<invalid utf8>")),
                Err(e) => kprintln!("ext2: hello.txt full read failed: {:?}", e),
            }

            // Reopen to reset offset, seek 5 bytes in, then read the next 5.
            let mut file = inode.fops.open(Arc::clone(&inode)).unwrap();
            // Seek 5 bytes in, then read the next 5.
            match fops.seek(&mut file, SeekFrom::Current(5)) {
                Ok(()) => {}
                Err(e) => { kprintln!("ext2: hello.txt seek failed: {:?}", e); return; }
            }
            let mut buf_a = [0u8; 5];
            match fops.read(&mut file, &mut buf_a) {
                Ok(n) => kprintln!("ext2: hello.txt [5..10]: {:?}", core::str::from_utf8(&buf_a[..n]).unwrap_or("<invalid utf8>")),
                Err(e) => kprintln!("ext2: hello.txt read [5..10] failed: {:?}", e),
            }

            // Read the remainder of the file.
            let mut buf_b = [0u8; 256];
            match fops.read(&mut file, &mut buf_b) {
                Ok(n) => kprintln!("ext2: hello.txt [10..]: {:?}", core::str::from_utf8(&buf_b[..n]).unwrap_or("<invalid utf8>")),
                Err(e) => kprintln!("ext2: hello.txt read [10..] failed: {:?}", e),
            }
        }
        Err(e) => kprintln!("ext2: lookup hello.txt failed: {:?}", e),
    }

    // Step 6: Inode cache verification.
    // Lookups [1] and [2] held simultaneously: same address (cache hit).
    // Lookup [3] after caller drops: same address — LRU Vec still holds strong Arc.
    // Lookup [4] of different inode: miss, inserts lost+found; count grows to 3
    //   (root + hello.txt + lost+found all pinned in LRU Vec).
    {
        let inode1 = root.iops.lookup(&root, "hello.txt");
        let inode2 = root.iops.lookup(&root, "hello.txt");
        if let Ok(i) = &inode1 { kprintln!("ext2: lookup[1] hello.txt: addr={:#x}", Arc::as_ptr(i) as usize); }
        if let Ok(i) = &inode2 { kprintln!("ext2: lookup[2] hello.txt: addr={:#x}", Arc::as_ptr(i) as usize); }
    }
    match root.iops.lookup(&root, "hello.txt") {
        Ok(i) => kprintln!("ext2: lookup[3] hello.txt: addr={:#x}", Arc::as_ptr(&i) as usize),
        Err(e) => kprintln!("ext2: lookup[3] hello.txt failed: {:?}", e),
    }
    match root.iops.lookup(&root, "lost+found") {
        Ok(i) => kprintln!("ext2: lookup[4] lost+found: addr={:#x}", Arc::as_ptr(&i) as usize),
        Err(e) => kprintln!("ext2: lookup[4] lost+found failed: {:?}", e),
    }
}

