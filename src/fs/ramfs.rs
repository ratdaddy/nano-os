//! Read-only RAM filesystem backed by static data.
//!
//! Provides a hierarchical filesystem that can be populated with static data.
//! The filesystem is built using `init()` and `insert_file()`, then accessed
//! via `open()` and `readdir()`.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use core::any::Any;

use crate::file::{DirEntry, Error, File, FileOps, FileType, Inode, SeekFrom, SuperBlock};

/// Filesystem-specific node data.
enum RamfsNode {
    /// Regular file with static data.
    File { data: &'static [u8] },
    /// Directory with named children.
    Dir { children: BTreeMap<String, &'static RamfsInode> },
    /// Character device with major/minor numbers.
    CharDevice { major: u32, minor: u32 },
}

/// Inode for ramfs, containing node type.
struct RamfsInode {
    node: RamfsNode,
}

impl Inode for RamfsInode {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn file_type(&self) -> FileType {
        match &self.node {
            RamfsNode::File { .. } => FileType::RegularFile,
            RamfsNode::Dir { .. } => FileType::Directory,
            RamfsNode::CharDevice { .. } => FileType::CharDevice,
        }
    }

    fn len(&self) -> usize {
        match &self.node {
            RamfsNode::File { data } => data.len(),
            RamfsNode::Dir { .. } | RamfsNode::CharDevice { .. } => 0,
        }
    }

    fn lookup(&self, name: &str) -> Result<&'static dyn Inode, Error> {
        match &self.node {
            RamfsNode::Dir { children } => {
                children.get(name).map(|&inode| inode as &'static dyn Inode).ok_or(Error::NotFound)
            }
            RamfsNode::File { .. } | RamfsNode::CharDevice { .. } => Err(Error::NotADirectory),
        }
    }

    fn file_ops(&self) -> &'static dyn FileOps {
        &RAMFS_FILE_OPS
    }

    fn rdev(&self) -> Option<(u32, u32)> {
        match &self.node {
            RamfsNode::CharDevice { major, minor } => Some((*major, *minor)),
            _ => None,
        }
    }

    fn superblock(&self) -> Option<&'static dyn SuperBlock> {
        unsafe {
            let sb = core::ptr::addr_of!(RAMFS_SB);
            *sb
        }
    }
}

/// File operations for ramfs files (read-only).
struct RamfsFileOps;

impl FileOps for RamfsFileOps {
    fn read(&self, file: &mut File, buf: &mut [u8]) -> Result<usize, Error> {
        let ramfs_inode = file.inode
            .as_any()
            .downcast_ref::<RamfsInode>()
            .ok_or(Error::InvalidInput)?;

        let data = match &ramfs_inode.node {
            RamfsNode::File { data } => *data,
            RamfsNode::Dir { .. } | RamfsNode::CharDevice { .. } => return Err(Error::InvalidInput),
        };

        let remaining = &data[file.offset..];
        let len = remaining.len().min(buf.len());
        buf[..len].copy_from_slice(&remaining[..len]);
        file.offset += len;
        Ok(len)
    }

    fn seek(&self, file: &mut File, pos: SeekFrom) -> Result<(), Error> {
        let file_len = file.inode.len();

        match pos {
            SeekFrom::Start(offset) => {
                if offset > file_len {
                    return Err(Error::UnexpectedEof);
                }
                file.offset = offset;
            }
            SeekFrom::Current(offset) => {
                let new_offset = file
                    .offset
                    .checked_add_signed(offset)
                    .ok_or(Error::InvalidInput)?;
                if new_offset > file_len {
                    return Err(Error::UnexpectedEof);
                }
                file.offset = new_offset;
            }
        }
        Ok(())
    }

    fn write(&self, _file: &mut File, _buf: &[u8]) -> Result<usize, Error> {
        // Read-only filesystem
        Err(Error::InvalidInput)
    }

    fn readdir(&self, file: &mut File) -> Result<alloc::vec::Vec<DirEntry>, Error> {
        let ramfs_inode = file.inode
            .as_any()
            .downcast_ref::<RamfsInode>()
            .ok_or(Error::InvalidInput)?;

        let children = match &ramfs_inode.node {
            RamfsNode::Dir { children } => children,
            RamfsNode::File { .. } | RamfsNode::CharDevice { .. } => return Err(Error::NotADirectory),
        };

        let entries = children
            .iter()
            .map(|(name, inode)| {
                let file_type = match &inode.node {
                    RamfsNode::Dir { .. } => FileType::Directory,
                    RamfsNode::File { .. } => FileType::RegularFile,
                    RamfsNode::CharDevice { .. } => FileType::CharDevice,
                };
                DirEntry { name: name.clone(), file_type }
            })
            .collect();

        Ok(entries)
    }
}

/// Static instance of RamfsFileOps for use with File.
static RAMFS_FILE_OPS: RamfsFileOps = RamfsFileOps;

// =============================================================================
// Ramfs Builder API
// =============================================================================

// =============================================================================
// Filesystem driver
// =============================================================================

pub struct RamfsFileSystem;

impl crate::vfs::FileSystem for RamfsFileSystem {
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

static mut RAMFS_SB: Option<&'static dyn SuperBlock> = None;

/// SuperBlock for ramfs.
pub struct RamfsSuperBlock {
    root: &'static RamfsInode,
}

impl SuperBlock for RamfsSuperBlock {
    fn root_inode(&self) -> &'static dyn Inode {
        self.root
    }

    fn fs_type(&self) -> &'static str {
        "ramfs"
    }
}

/// A ramfs filesystem instance.
pub struct Ramfs {
    root: &'static RamfsInode,
}

impl Ramfs {
    /// Create a new ramfs with an empty root directory.
    pub fn new() -> Self {
        let root = Box::leak(Box::new(RamfsInode {
            node: RamfsNode::Dir {
                children: BTreeMap::new(),
            },
        }));
        Self { root }
    }

    /// Get the root inode.
    #[allow(dead_code)]
    pub fn root(&self) -> &'static dyn Inode {
        self.root
    }

    /// Create a SuperBlock for this ramfs instance.
    pub fn superblock(&self) -> &'static RamfsSuperBlock {
        let sb: &'static RamfsSuperBlock = Box::leak(Box::new(RamfsSuperBlock { root: self.root }));
        unsafe {
            let ptr = core::ptr::addr_of_mut!(RAMFS_SB);
            *ptr = Some(sb);
        }
        sb
    }

    /// Insert an empty directory, creating parent directories as needed.
    pub fn insert_dir(&self, path: &str) -> Result<(), Error> {
        let parts: alloc::vec::Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return Err(Error::InvalidInput);
        }

        let mut current = self.root;
        for &dir_name in &parts {
            current = self.get_or_create_dir(current, dir_name)?;
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

        // Navigate/create parent directories
        let mut current = self.root;
        for &dir_name in dirs {
            current = self.get_or_create_dir(current, dir_name)?;
        }

        // Create file inode
        let file_inode = Box::leak(Box::new(RamfsInode {
            node: RamfsNode::File { data },
        }));

        // Insert file into current directory
        let current_ptr = current as *const RamfsInode as *mut RamfsInode;
        unsafe {
            if let RamfsNode::Dir { children } = &mut (*current_ptr).node {
                children.insert(String::from(filename), file_inode);
            }
        }
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

        let mut current = self.root;
        for &dir_name in dirs {
            current = self.get_or_create_dir(current, dir_name)?;
        }

        let dev_inode = Box::leak(Box::new(RamfsInode {
            node: RamfsNode::CharDevice { major, minor },
        }));

        let current_ptr = current as *const RamfsInode as *mut RamfsInode;
        unsafe {
            if let RamfsNode::Dir { children } = &mut (*current_ptr).node {
                children.insert(String::from(filename), dev_inode);
            }
        }
        Ok(())
    }

    fn get_or_create_dir(
        &self,
        parent: &'static RamfsInode,
        name: &str,
    ) -> Result<&'static RamfsInode, Error> {
        let parent_ptr = parent as *const RamfsInode as *mut RamfsInode;

        unsafe {
            if let RamfsNode::Dir { children } = &mut (*parent_ptr).node {
                if let Some(&existing) = children.get(name) {
                    return Ok(existing);
                }
                let new_dir = Box::leak(Box::new(RamfsInode {
                    node: RamfsNode::Dir {
                        children: BTreeMap::new(),
                    },
                }));
                children.insert(String::from(name), new_dir);
                Ok(new_dir)
            } else {
                Err(Error::InvalidInput)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;
    use super::*;

    /// Create static test data from a byte slice.
    fn leak_data(data: &[u8]) -> &'static [u8] {
        Box::leak(data.to_vec().into_boxed_slice())
    }

    fn setup_test_ramfs() -> &'static Ramfs {
        Box::leak(Box::new(Ramfs::new()))
    }

    /// Look up an inode by path from the root.
    fn lookup(root: &'static dyn Inode, path: &str) -> Result<&'static dyn Inode, Error> {
        let mut inode = root;
        for component in path.split('/').filter(|s| !s.is_empty()) {
            inode = inode.lookup(component)?;
        }
        Ok(inode)
    }

    /// Open a file by path.
    fn open(root: &'static dyn Inode, path: &str) -> Result<File, Error> {
        let inode = lookup(root, path)?;
        inode.file_ops().open(inode)
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
    fn readdir(root: &'static dyn Inode, path: &str) -> Result<alloc::vec::Vec<DirEntry>, Error> {
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
        let result = seek(&mut file, SeekFrom::Start(1000));
        assert!(matches!(result, Err(Error::UnexpectedEof)));
    }

    #[test_case]
    fn test_seek_negative() {
        println!("Testing ramfs seek negative...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_file("back.txt", leak_data(b"12345678")).unwrap();

        let mut file = open(ramfs.root(), "back.txt").unwrap();
        let result = seek(&mut file, SeekFrom::Current(-10));
        assert!(matches!(result, Err(Error::InvalidInput)));
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
        assert_eq!(inode.rdev(), Some((5, 1)));
    }

    // -- lookup error tests --

    #[test_case]
    fn test_lookup_not_found() {
        println!("Testing ramfs lookup non-existent child...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_file("a.txt", leak_data(b"data")).unwrap();

        let result = ramfs.root().lookup("nonexistent");
        assert!(matches!(result, Err(Error::NotFound)));
    }

    #[test_case]
    fn test_lookup_on_file() {
        println!("Testing ramfs lookup on a file inode...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_file("a.txt", leak_data(b"data")).unwrap();

        let file_inode = ramfs.root().lookup("a.txt").unwrap();
        let result = file_inode.lookup("child");
        assert!(matches!(result, Err(Error::NotADirectory)));
    }

    #[test_case]
    fn test_lookup_on_chardev() {
        println!("Testing ramfs lookup on a chardev inode...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_chardev("dev/console", 5, 1).unwrap();

        let dev = ramfs.root().lookup("dev").unwrap();
        let console = dev.lookup("console").unwrap();
        let result = console.lookup("child");
        assert!(matches!(result, Err(Error::NotADirectory)));
    }

    // -- rdev tests --

    #[test_case]
    fn test_rdev_on_file() {
        println!("Testing ramfs rdev on a file inode...");

        let ramfs = setup_test_ramfs();
        ramfs.insert_file("a.txt", leak_data(b"data")).unwrap();

        let file_inode = ramfs.root().lookup("a.txt").unwrap();
        assert_eq!(file_inode.rdev(), None);
    }

    #[test_case]
    fn test_rdev_on_directory() {
        println!("Testing ramfs rdev on a directory inode...");

        let ramfs = setup_test_ramfs();
        assert_eq!(ramfs.root().rdev(), None);
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
