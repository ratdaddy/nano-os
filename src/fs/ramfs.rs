//! Read-only RAM filesystem backed by static data.
//!
//! Provides a hierarchical filesystem that can be populated with static data.
//! The filesystem is built using `init()` and `insert_file()`, then accessed
//! via `open()` and `readdir()`.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::file::{DirEntry, Error, File, FileOps, FileType, Inode, InodeOps, SuperBlock};
use crate::vfs::FileSystem;

/// Filesystem-specific node data stored in each inode's `fs_data`.
enum RamfsNode {
    /// Regular file with static data.
    File { data: &'static [u8] },
    /// Directory with named children.
    ///
    /// `UnsafeCell` allows inserting children during single-threaded init
    /// without a raw-pointer cast. After mount the filesystem is read-only.
    Dir { children: UnsafeCell<BTreeMap<String, Arc<Inode>>> },
    /// Character device. Major/minor numbers are stored in `inode.rdev`.
    CharDevice,
}

// Safety: RamfsNode is only mutated during single-threaded initialisation.
// After the Ramfs is mounted all accesses are read-only.
unsafe impl Send for RamfsNode {}
unsafe impl Sync for RamfsNode {}

// =============================================================================
// Ops tables
// =============================================================================

struct RamfsInodeOps;
struct RamfsFileOps;

static RAMFS_INODE_OPS: RamfsInodeOps = RamfsInodeOps;
static RAMFS_FILE_OPS: RamfsFileOps = RamfsFileOps;

impl InodeOps for RamfsInodeOps {
    fn lookup(&self, inode: &Arc<Inode>, name: &str) -> Result<Arc<Inode>, Error> {
        let node = inode.fs_data.downcast_ref::<RamfsNode>().unwrap();
        match node {
            RamfsNode::Dir { children } => {
                let children = unsafe { &*children.get() };
                children.get(name).cloned().ok_or(Error::NotFound)
            }
            RamfsNode::File { .. } | RamfsNode::CharDevice => Err(Error::NotADirectory),
        }
    }
}

impl FileOps for RamfsFileOps {
    fn read(&self, file: &mut File, buf: &mut [u8]) -> Result<usize, Error> {
        let node = file.inode.fs_data.downcast_ref::<RamfsNode>().ok_or(Error::InvalidInput)?;
        let data = match node {
            RamfsNode::File { data } => *data,
            RamfsNode::Dir { .. } | RamfsNode::CharDevice => return Err(Error::InvalidInput),
        };
        let remaining = &data[file.offset..];
        let len = remaining.len().min(buf.len());
        buf[..len].copy_from_slice(&remaining[..len]);
        file.offset += len;
        Ok(len)
    }


    fn write(&self, _file: &mut File, _buf: &[u8]) -> Result<usize, Error> {
        // Read-only filesystem
        Err(Error::InvalidInput)
    }

    fn readdir(&self, file: &mut File) -> Result<alloc::vec::Vec<DirEntry>, Error> {
        let node = file.inode.fs_data.downcast_ref::<RamfsNode>().ok_or(Error::InvalidInput)?;
        match node {
            RamfsNode::Dir { children } => {
                let children = unsafe { &*children.get() };
                let entries = children
                    .iter()
                    .map(|(name, inode)| DirEntry { name: name.clone(), file_type: inode.file_type })
                    .collect();
                Ok(entries)
            }
            RamfsNode::File { .. } | RamfsNode::CharDevice => Err(Error::NotADirectory),
        }
    }
}

// =============================================================================
// Filesystem driver
// =============================================================================

pub struct RamfsFileSystem;

impl FileSystem for RamfsFileSystem {
    fn name(&self) -> &'static str { "ramfs" }
    fn requires_device(&self) -> bool { false }
    fn mount(&self) -> Result<&'static dyn SuperBlock, Error> {
        let ramfs = Box::leak(Box::new(Ramfs::new()));
        Ok(ramfs.superblock())
    }
}

pub static RAMFS_FS: RamfsFileSystem = RamfsFileSystem;

// =============================================================================
// SuperBlock
// =============================================================================

/// SuperBlock for ramfs.
pub struct RamfsSuperBlock {
    root: Arc<Inode>,
}

impl SuperBlock for RamfsSuperBlock {
    fn root_inode(&self) -> Arc<Inode> {
        Arc::clone(&self.root)
    }

    fn fs_type(&self) -> &'static str {
        "ramfs"
    }
}

// =============================================================================
// Ramfs Builder API
// =============================================================================

/// A ramfs filesystem instance.
pub struct Ramfs {
    root: Arc<Inode>,
    next_ino: AtomicU64,
}

impl Ramfs {
    /// Create a new ramfs with an empty root directory.
    pub fn new() -> Self {
        Self {
            root: Self::make_dir_inode(1),
            next_ino: AtomicU64::new(2),
        }
    }

    fn alloc_ino(&self) -> u64 {
        self.next_ino.fetch_add(1, Ordering::Relaxed)
    }

    /// Get the root inode.
    #[allow(dead_code)]
    pub fn root(&self) -> Arc<Inode> {
        Arc::clone(&self.root)
    }

    /// Create a SuperBlock for this ramfs instance.
    pub fn superblock(&self) -> &'static RamfsSuperBlock {
        Box::leak(Box::new(RamfsSuperBlock { root: Arc::clone(&self.root) }))
    }

    /// Insert an empty directory, creating parent directories as needed.
    pub fn insert_dir(&self, path: &str) -> Result<(), Error> {
        let parts: alloc::vec::Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return Err(Error::InvalidInput);
        }
        let mut current = Arc::clone(&self.root);
        for &dir_name in &parts {
            current = self.get_or_create_dir(&current, dir_name)?;
        }
        Ok(())
    }

    /// Insert a file, creating parent directories as needed.
    pub fn insert_file(&self, path: &str, data: &'static [u8]) -> Result<(), Error> {
        let parts: alloc::vec::Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return Err(Error::InvalidInput);
        }
        let (dirs, filename) = parts.split_at(parts.len() - 1);
        let filename = filename[0];
        let mut current = Arc::clone(&self.root);
        for &dir_name in dirs {
            current = self.get_or_create_dir(&current, dir_name)?;
        }
        let file_inode = self.make_file_inode(data);
        Self::dir_insert(&current, String::from(filename), file_inode);
        Ok(())
    }

    /// Insert a character device node, creating parent directories as needed.
    pub fn insert_chardev(&self, path: &str, major: u32, minor: u32) -> Result<(), Error> {
        let parts: alloc::vec::Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return Err(Error::InvalidInput);
        }
        let (dirs, filename) = parts.split_at(parts.len() - 1);
        let filename = filename[0];
        let mut current = Arc::clone(&self.root);
        for &dir_name in dirs {
            current = self.get_or_create_dir(&current, dir_name)?;
        }
        let dev_inode = self.make_chardev_inode(major, minor);
        Self::dir_insert(&current, String::from(filename), dev_inode);
        Ok(())
    }

    fn make_dir_inode(ino: u64) -> Arc<Inode> {
        Arc::new(Inode {
            ino,
            file_type: FileType::Directory,
            len: 0,
            iops: &RAMFS_INODE_OPS,
            fops: &RAMFS_FILE_OPS,
            sb: None,
            rdev: None,
            fs_data: Box::new(RamfsNode::Dir { children: UnsafeCell::new(BTreeMap::new()) }),
        })
    }

    fn make_file_inode(&self, data: &'static [u8]) -> Arc<Inode> {
        Arc::new(Inode {
            ino: self.alloc_ino(),
            file_type: FileType::RegularFile,
            len: data.len(),
            iops: &RAMFS_INODE_OPS,
            fops: &RAMFS_FILE_OPS,
            sb: None,
            rdev: None,
            fs_data: Box::new(RamfsNode::File { data }),
        })
    }

    fn make_chardev_inode(&self, major: u32, minor: u32) -> Arc<Inode> {
        Arc::new(Inode {
            ino: self.alloc_ino(),
            file_type: FileType::CharDevice,
            len: 0,
            iops: &RAMFS_INODE_OPS,
            fops: &RAMFS_FILE_OPS,
            sb: None,
            rdev: Some((major, minor)),
            fs_data: Box::new(RamfsNode::CharDevice),
        })
    }

    fn get_or_create_dir(&self, parent: &Arc<Inode>, name: &str) -> Result<Arc<Inode>, Error> {
        let node = parent.fs_data.downcast_ref::<RamfsNode>().unwrap();
        if let RamfsNode::Dir { children } = node {
            // Safety: single-threaded initialisation; no concurrent readers yet.
            let children = unsafe { &mut *children.get() };
            if let Some(existing) = children.get(name) {
                return Ok(Arc::clone(existing));
            }
            let new_dir = Self::make_dir_inode(self.alloc_ino());
            children.insert(String::from(name), Arc::clone(&new_dir));
            Ok(new_dir)
        } else {
            Err(Error::InvalidInput)
        }
    }

    fn dir_insert(parent: &Arc<Inode>, name: String, child: Arc<Inode>) {
        let node = parent.fs_data.downcast_ref::<RamfsNode>().unwrap();
        if let RamfsNode::Dir { children } = node {
            // Safety: single-threaded initialisation; no concurrent readers yet.
            let children = unsafe { &mut *children.get() };
            children.insert(name, child);
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;
    use alloc::sync::Arc;
    use super::*;
    use crate::file::SeekFrom;

    /// Create static test data from a byte slice.
    fn leak_data(data: &[u8]) -> &'static [u8] {
        Box::leak(data.to_vec().into_boxed_slice())
    }

    fn setup_test_ramfs() -> &'static Ramfs {
        Box::leak(Box::new(Ramfs::new()))
    }

    /// Look up an inode by path from the root.
    fn lookup(root: Arc<Inode>, path: &str) -> Result<Arc<Inode>, Error> {
        let mut inode = root;
        for component in path.split('/').filter(|s| !s.is_empty()) {
            let next = inode.iops.lookup(&inode, component)?;
            inode = next;
        }
        Ok(inode)
    }

    /// Open a file by path.
    fn open(root: Arc<Inode>, path: &str) -> Result<File, Error> {
        let inode = lookup(root, path)?;
        let fops = inode.fops;
        fops.open(inode)
    }

    /// Read from a file.
    fn read(file: &mut File, buf: &mut [u8]) -> Result<usize, Error> {
        let ops = file.fops;
        ops.read(file, buf)
    }

    /// Seek in a file.
    fn seek(file: &mut File, pos: SeekFrom) -> Result<(), Error> {
        let ops = file.fops;
        ops.seek(file, pos)
    }

    /// Read directory entries by path.
    fn readdir(root: Arc<Inode>, path: &str) -> Result<alloc::vec::Vec<DirEntry>, Error> {
        let mut file = open(root, path)?;
        let ops = file.fops;
        ops.readdir(&mut file)
    }

    #[test_case]
    fn test_seek_and_read() {
        println!("Testing ramfs seek and read...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_file("test.txt", leak_data(b"hello world")).unwrap();

        let mut file = open(ramfs.root(), "test.txt").unwrap();

        let mut buf = [0u8; 5];
        assert_eq!(read(&mut file, &mut buf).unwrap(), 5);
        assert_eq!(&buf, b"hello");

        seek(&mut file, SeekFrom::Start(6)).unwrap();
        assert_eq!(read(&mut file, &mut buf).unwrap(), 5);
        assert_eq!(&buf, b"world");
    }

    #[test_case]
    fn test_seek_beyond_end() {
        println!("Testing ramfs seek beyond end...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_file("tiny.txt", leak_data(b"short")).unwrap();

        let mut file = open(ramfs.root(), "tiny.txt").unwrap();
        seek(&mut file, SeekFrom::Start(1000)).unwrap();
        assert_eq!(file.offset, 5); // clamped to file_len
    }

    #[test_case]
    fn test_seek_negative() {
        println!("Testing ramfs seek negative...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_file("back.txt", leak_data(b"12345678")).unwrap();

        let mut file = open(ramfs.root(), "back.txt").unwrap();
        seek(&mut file, SeekFrom::Current(-10)).unwrap();
        assert_eq!(file.offset, 0); // clamped to 0
    }

    #[test_case]
    fn test_nested_directories() {
        println!("Testing ramfs nested directories...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_file("etc/motd", leak_data(b"Welcome!")).unwrap();
        ramfs.insert_file("etc/hosts", leak_data(b"127.0.0.1 localhost")).unwrap();

        let mut file = open(ramfs.root(), "etc/motd").unwrap();
        let mut buf = [0u8; 8];
        assert_eq!(read(&mut file, &mut buf).unwrap(), 8);
        assert_eq!(&buf, b"Welcome!");

        let entries = readdir(ramfs.root(), "etc").unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test_case]
    fn test_readdir_root() {
        println!("Testing ramfs readdir root...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_file("file1.txt", leak_data(b"one")).unwrap();
        ramfs.insert_file("subdir/file2.txt", leak_data(b"two")).unwrap();

        let entries = readdir(ramfs.root(), "/").unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test_case]
    fn test_insert_empty_dir() {
        println!("Testing ramfs insert empty directory...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_dir("mnt").unwrap();

        let entries = readdir(ramfs.root(), "/").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "mnt");
        assert_eq!(entries[0].file_type, FileType::Directory);

        let mnt_entries = readdir(ramfs.root(), "mnt").unwrap();
        assert_eq!(mnt_entries.len(), 0);
    }

    #[test_case]
    fn test_insert_nested_empty_dir() {
        println!("Testing ramfs insert nested empty directory...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_dir("mnt/usb").unwrap();

        let root_entries = readdir(ramfs.root(), "/").unwrap();
        assert_eq!(root_entries.len(), 1);
        assert_eq!(root_entries[0].name, "mnt");

        let mnt_entries = readdir(ramfs.root(), "mnt").unwrap();
        assert_eq!(mnt_entries.len(), 1);
        assert_eq!(mnt_entries[0].name, "usb");

        let usb_entries = readdir(ramfs.root(), "mnt/usb").unwrap();
        assert_eq!(usb_entries.len(), 0);
    }

    #[test_case]
    fn test_insert_dir_existing() {
        println!("Testing ramfs insert_dir on existing directory...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_file("etc/motd", leak_data(b"Welcome!")).unwrap();
        ramfs.insert_dir("etc").unwrap();

        let entries = readdir(ramfs.root(), "etc").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "motd");
    }

    #[test_case]
    fn test_insert_chardev() {
        println!("Testing ramfs insert chardev...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_chardev("dev/console", 5, 1).unwrap();

        let entries = readdir(ramfs.root(), "dev").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "console");
        assert_eq!(entries[0].file_type, FileType::CharDevice);

        let inode = lookup(ramfs.root(), "dev/console").unwrap();
        assert_eq!(inode.rdev, Some((5, 1)));
    }

    // -- lookup error tests --

    #[test_case]
    fn test_lookup_not_found() {
        println!("Testing ramfs lookup non-existent child...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_file("a.txt", leak_data(b"data")).unwrap();

        let root = ramfs.root();
        let result = root.iops.lookup(&root, "nonexistent");
        assert!(matches!(result, Err(Error::NotFound)));
    }

    #[test_case]
    fn test_lookup_on_file() {
        println!("Testing ramfs lookup on a file inode...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_file("a.txt", leak_data(b"data")).unwrap();

        let file_inode = lookup(ramfs.root(), "a.txt").unwrap();
        let result = file_inode.iops.lookup(&file_inode, "child");
        assert!(matches!(result, Err(Error::NotADirectory)));
    }

    #[test_case]
    fn test_lookup_on_chardev() {
        println!("Testing ramfs lookup on a chardev inode...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_chardev("dev/console", 5, 1).unwrap();

        let console = lookup(ramfs.root(), "dev/console").unwrap();
        let result = console.iops.lookup(&console, "child");
        assert!(matches!(result, Err(Error::NotADirectory)));
    }

    // -- ino tests --

    #[test_case]
    fn test_ino_root() {
        println!("Testing ramfs root inode has ino=1...");

        let ramfs = setup_test_ramfs();
        assert_eq!(ramfs.root().ino, 1);
    }

    #[test_case]
    fn test_ino_sequential() {
        println!("Testing ramfs assigns sequential inode numbers...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_file("file.txt", leak_data(b"data")).unwrap();
        ramfs.insert_dir("mydir").unwrap();
        ramfs.insert_chardev("dev", 5, 1).unwrap();

        let file_ino = lookup(ramfs.root(), "file.txt").unwrap().ino;
        let dir_ino  = lookup(ramfs.root(), "mydir").unwrap().ino;
        let dev_ino  = lookup(ramfs.root(), "dev").unwrap().ino;

        assert_eq!(file_ino, 2);
        assert_eq!(dir_ino,  3);
        assert_eq!(dev_ino,  4);
    }

    // -- rdev tests --

    #[test_case]
    fn test_rdev_on_file() {
        println!("Testing ramfs rdev on a file inode...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_file("a.txt", leak_data(b"data")).unwrap();

        let file_inode = lookup(ramfs.root(), "a.txt").unwrap();
        assert_eq!(file_inode.rdev, None);
    }

    #[test_case]
    fn test_rdev_on_directory() {
        println!("Testing ramfs rdev on a directory inode...");

        let ramfs = setup_test_ramfs();
        assert_eq!(ramfs.root().rdev, None);
    }

    // -- read error tests --

    #[test_case]
    fn test_read_directory() {
        println!("Testing ramfs read on a directory...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_dir("mydir").unwrap();

        let mut file = open(ramfs.root(), "mydir").unwrap();
        let mut buf = [0u8; 4];
        let result = read(&mut file, &mut buf);
        assert!(matches!(result, Err(Error::InvalidInput)));
    }

    #[test_case]
    fn test_read_chardev() {
        println!("Testing ramfs read on a chardev...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_chardev("dev/console", 5, 1).unwrap();

        let mut file = open(ramfs.root(), "dev/console").unwrap();
        let mut buf = [0u8; 4];
        let result = read(&mut file, &mut buf);
        assert!(matches!(result, Err(Error::InvalidInput)));
    }

    // -- readdir error tests --

    #[test_case]
    fn test_readdir_on_file() {
        println!("Testing ramfs readdir on a file...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_file("a.txt", leak_data(b"data")).unwrap();

        let result = readdir(ramfs.root(), "a.txt");
        assert!(matches!(result, Err(Error::NotADirectory)));
    }

    #[test_case]
    fn test_readdir_on_chardev() {
        println!("Testing ramfs readdir on a chardev...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_chardev("dev/console", 5, 1).unwrap();

        let result = readdir(ramfs.root(), "dev/console");
        assert!(matches!(result, Err(Error::NotADirectory)));
    }

    #[test_case]
    fn test_readdir_mixed_types() {
        println!("Testing ramfs readdir with mixed node types...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_chardev("console", 5, 1).unwrap();
        ramfs.insert_file("hello.txt", leak_data(b"hi")).unwrap();
        ramfs.insert_dir("subdir").unwrap();

        let entries = readdir(ramfs.root(), "/").unwrap();
        assert_eq!(entries.len(), 3);

        // BTreeMap sorts by name
        assert_eq!(entries[0].name, "console");
        assert_eq!(entries[0].file_type, FileType::CharDevice);
        assert_eq!(entries[1].name, "hello.txt");
        assert_eq!(entries[1].file_type, FileType::RegularFile);
        assert_eq!(entries[2].name, "subdir");
        assert_eq!(entries[2].file_type, FileType::Directory);
    }
}
