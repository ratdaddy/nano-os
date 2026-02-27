//! ext2 filesystem implementation
//!
//! Read-only ext2 filesystem driver.

use alloc::alloc::{alloc, Layout};
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
use crate::file::{DirEntry, Error, File, FileOps, FileType, Inode, InodeOps, SuperBlock};
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

/// Combined inode cache: a BTreeMap index for O(log n) lookup and a Vec LRU
/// list that holds strong `Arc`s to keep the most recently used inodes alive.
struct InodeLruCache {
    index: BTreeMap<u32, Weak<Inode>>,
    lru: Vec<Arc<Inode>>,
}

impl InodeLruCache {
    fn new() -> Self {
        Self { index: BTreeMap::new(), lru: Vec::new() }
    }

    /// Look up an inode, moving it to the front of the LRU on a hit.
    ///
    /// If the entry was evicted from the Vec but the Weak is still live (e.g. held
    /// by an open file), it is re-inserted at the front as if it were a new entry.
    fn get(&mut self, inode_num: u32) -> Option<Arc<Inode>> {
        let arc = self.index.get(&inode_num)?.upgrade()?;
        if let Some(pos) = self.lru.iter().position(|e| e.ino == inode_num as u64) {
            let entry = self.lru.remove(pos);
            self.lru.insert(0, entry);
        } else {
            self.lru.insert(0, Arc::clone(&arc));
            if self.lru.len() > INODE_CACHE_CAPACITY {
                self.lru.pop();
            }
        }
        Some(arc)
    }

    /// Insert an inode, evicting the LRU entry if at capacity and reaping dead Weaks.
    fn insert(&mut self, inode_num: u32, inode: Arc<Inode>) {
        self.index.insert(inode_num, Arc::downgrade(&inode));
        self.lru.insert(0, inode);
        if self.lru.len() > INODE_CACHE_CAPACITY {
            self.lru.pop();
        }
        self.index.retain(|_, weak| weak.upgrade().is_some());
    }

    fn len(&self) -> usize {
        self.index.len()
    }
}

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
    buf: Box<Ext2BlockBuffer>,
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
            // Allocate directly on the heap — avoid creating a 4096-byte
            // aligned local on the stack, which can overflow the 32KB thread stack.
            buf: {
                let layout = Layout::new::<Ext2BlockBuffer>();
                let ptr = unsafe { alloc(layout) };
                assert!(!ptr.is_null(), "Failed to allocate Ext2BlockBuffer");
                unsafe { Box::from_raw(ptr as *mut Ext2BlockBuffer) }
            },
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
                let sector = (block_ptr as u64 * self.block_size as u64) / BLOCK_SIZE as u64;
                if let Err(e) = self.sb.volume.as_ref().read_blocks(sector, &mut self.buf.0) {
                    return Some(Err(e));
                }
                self.block_offset = 0;
            }

            let offset = self.block_offset;

            if offset + DIR_ENTRY_NAME_OFFSET > self.block_size {
                // Misaligned — block is likely corrupt; skip to the next one.
                self.block_idx   += 1;
                self.block_offset = self.block_size;
                continue;
            }

            let ino      = self.buf.0.read_u32_le(offset + DIR_ENTRY_INODE_OFFSET);
            let rec_len  = self.buf.0.read_u16_le(offset + DIR_ENTRY_REC_LEN_OFFSET) as usize;
            let name_len = self.buf.0[offset + DIR_ENTRY_NAME_LEN_OFFSET] as usize;
            let ft_byte  = self.buf.0[offset + DIR_ENTRY_FILE_TYPE_OFFSET];

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

            match from_utf8(&self.buf.0[name_start..name_end]) {
                Ok(name) => return Some(Ok((ino, String::from(name), ft_byte))),
                Err(_)   => continue, // non-UTF-8 name; skip
            }
        }
    }
}

// =============================================================================
// I/O buffer
// =============================================================================

// Page-aligned buffer for ext2 block I/O (4KB).
//
// align(4096) ensures the buffer never crosses a physical page boundary, which
// is required for DMA since virtual-to-physical mappings are page-granular.
// Used as a static (IO_BUFFER) for superblock/inode reads and as a heap-allocated
// Box (DirEntryIter::buf) for directory block reads.
//
// Block size strategy: always read a full 4KB even if the filesystem uses smaller
// blocks (1KB/2KB). The extra data is ignored; this simplifies the code and works
// correctly because all valid block sizes divide 4096 evenly.
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
const DIR_ENTRY_FILE_TYPE_OFFSET: usize = 7;
const DIR_ENTRY_NAME_OFFSET: usize = 8;

// Directory entry file type values
const EXT2_FT_REG_FILE: u8 = 1;
const EXT2_FT_DIR: u8 = 2;
const EXT2_FT_CHRDEV: u8 = 3;

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
    inode_cache: UnsafeCell<InodeLruCache>,
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
            inode_cache: UnsafeCell::new(InodeLruCache::new()),
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
        if let Some(arc) = cache.get(inode_num) {
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
    let mut root_file = EXT2_FILE_OPS.open(Arc::clone(&root)).unwrap();
    match EXT2_FILE_OPS.readdir(&mut root_file) {
        Ok(entries) => {
            kprintln!("ext2: root directory ({} entries):", entries.len());
            for entry in &entries {
                let type_char = match entry.file_type {
                    FileType::Directory   => 'd',
                    FileType::RegularFile => 'f',
                    FileType::CharDevice  => 'c',
                };
                kprintln!("  {} {}", type_char, entry.name);
            }
        }
        Err(e) => kprintln!("ext2: readdir failed: {:?}", e),
    }

    // Step 5: Inode cache verification.
    // Lookups [1] and [2] held simultaneously: same address (cache hit).
    // Lookup [3] after caller drops: same address — LRU Vec still holds strong Arc.
    // Lookup [4] of different inode: miss, inserts lost+found; count grows to 3
    //   (root + hello.txt + lost+found all pinned in LRU Vec).
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

