//! Virtual File System operations.
//!
//! Provides the kernel's file operation API. The VFS caches a root inode
//! and uses inode operations for path traversal.

use alloc::vec::Vec;

use crate::file::{DirEntry, Error, File, FileType, Inode, SeekFrom};

// =============================================================================
// SuperBlock and Mount Table
// =============================================================================

/// SuperBlock trait — each filesystem provides one per mount.
pub trait SuperBlock: Send + Sync {
    fn root_inode(&self) -> &'static dyn Inode;
    fn fs_type(&self) -> &'static str;
}

/// A single mount record.
struct Mount {
    mountpoint_inode: Option<&'static dyn Inode>,
    mountpoint: &'static str,
    sb: &'static dyn SuperBlock,
}

/// Flattened view of a mount for consumers (boot menu, /proc/mounts).
pub struct MountInfo {
    pub id: usize,
    pub fs_type: &'static str,
    pub mountpoint: &'static str,
}

static mut MOUNTS: Option<Vec<Mount>> = None;

/// Initialize the VFS with a root filesystem SuperBlock (mount 0).
pub fn init(sb: &'static dyn SuperBlock) {
    unsafe {
        let mounts = core::ptr::addr_of_mut!(MOUNTS);
        *mounts = Some(Vec::new());
        (*mounts).as_mut().unwrap().push(Mount { mountpoint_inode: None, mountpoint: "/", sb });
    }
}

/// Return mount info for all mounted filesystems.
pub fn mounts() -> Vec<MountInfo> {
    unsafe {
        let mounts = core::ptr::addr_of!(MOUNTS);
        (*mounts).as_ref().map_or_else(Vec::new, |mounts| {
            mounts.iter().enumerate().map(|(i, m)| MountInfo {
                id: i,
                fs_type: m.sb.fs_type(),
                mountpoint: m.mountpoint,
            }).collect()
        })
    }
}

/// Mount a registered filesystem at the given path.
pub fn vfs_mount_at(path: &'static str, fs_name: &str) -> Result<(), Error> {
    let inode = vfs_lookup(path)?;
    let fs = find_filesystem(fs_name).ok_or(Error::NotFound)?;
    let sb = fs.mount()?;
    unsafe {
        let mounts = core::ptr::addr_of_mut!(MOUNTS);
        (*mounts).as_mut().expect("VFS not initialized").push(Mount {
            mountpoint_inode: Some(inode),
            mountpoint: path,
            sb,
        });
    }
    Ok(())
}

// =============================================================================
// Filesystem Registry
// =============================================================================

/// Filesystem driver trait — each filesystem type implements this.
pub trait FileSystem: Send + Sync {
    fn name(&self) -> &'static str;
    fn mount(&self) -> Result<&'static dyn SuperBlock, Error>;
}

static mut FILESYSTEMS: Option<Vec<&'static dyn FileSystem>> = None;

/// Register a filesystem driver.
pub fn register_filesystem(fs: &'static dyn FileSystem) {
    unsafe {
        let fss = core::ptr::addr_of_mut!(FILESYSTEMS);
        if (*fss).is_none() {
            *fss = Some(Vec::new());
        }
        (*fss).as_mut().unwrap().push(fs);
    }
}

/// Look up a registered filesystem by name.
pub fn find_filesystem(name: &str) -> Option<&'static dyn FileSystem> {
    unsafe {
        let fss = core::ptr::addr_of!(FILESYSTEMS);
        (*fss).as_ref()
            .and_then(|fss| fss.iter().find(|fs| fs.name() == name))
            .copied()
    }
}

/// Return names of all registered filesystems.
pub fn filesystems() -> Vec<&'static str> {
    unsafe {
        let fss = core::ptr::addr_of!(FILESYSTEMS);
        (*fss).as_ref().map_or_else(Vec::new, |fss| {
            fss.iter().map(|fs| fs.name()).collect()
        })
    }
}

// =============================================================================
// Path Lookup and File Operations
// =============================================================================

/// Return the root inode from mount 0.
fn root_inode() -> Result<&'static dyn Inode, Error> {
    unsafe {
        let mounts = core::ptr::addr_of!(MOUNTS);
        Ok((*mounts).as_ref().ok_or(Error::InvalidInput)?[0].sb.root_inode())
    }
}

/// Look up an inode by path without opening it.
pub fn vfs_lookup(path: &str) -> Result<&'static dyn Inode, Error> {
    let mut inode = root_inode()?;
    for component in path.split('/').filter(|s| !s.is_empty()) {
        inode = inode.lookup(component)?;
    }
    Ok(inode)
}

/// Open a file by path.
pub fn vfs_open(path: &str) -> Result<File, Error> {
    let inode = vfs_lookup(path)?;
    if inode.file_type() == FileType::CharDevice {
        return crate::chardev::chrdev_open(inode);
    }
    let fops = inode.file_ops();
    fops.open(inode)
}

/// Read directory entries.
pub fn vfs_readdir(path: &str) -> Result<Vec<DirEntry>, Error> {
    let mut file = vfs_open(path)?;
    let ops = file.fops;
    ops.readdir(&mut file)
}

/// Read from a file into a buffer.
/// Returns the number of bytes read (0 indicates EOF).
pub fn vfs_read(file: &mut File, buf: &mut [u8]) -> Result<usize, Error> {
    let ops = file.fops;  // Copy fat pointer (no borrow held)
    ops.read(file, buf)
}

/// Seek to a position in a file.
#[cfg_attr(test, allow(dead_code))]
pub fn vfs_seek(file: &mut File, pos: SeekFrom) -> Result<(), Error> {
    let ops = file.fops;
    ops.seek(file, pos)
}

/// Read exactly `buf.len()` bytes from a file.
/// Returns an error if EOF is reached before the buffer is filled.
pub fn vfs_read_exact(file: &mut File, buf: &mut [u8]) -> Result<(), Error> {
    let mut remaining = buf;
    while !remaining.is_empty() {
        let n = vfs_read(file, remaining)?;
        if n == 0 {
            return Err(Error::UnexpectedEof);
        }
        remaining = &mut remaining[n..];
    }
    Ok(())
}

/// Read entire file into a string.
pub fn vfs_read_to_string(file: &mut File, out: &mut alloc::string::String) -> Result<(), Error> {
    let mut buf = [0u8; 256];
    loop {
        let len = vfs_read(file, &mut buf)?;
        if len == 0 {
            break;
        }
        let s = core::str::from_utf8(&buf[..len]).map_err(|_| Error::InvalidUtf8)?;
        out.push_str(s);
    }
    Ok(())
}

/// Write a buffer to a file.
/// Returns the number of bytes written.
#[cfg_attr(test, allow(dead_code))]
pub fn vfs_write(file: &mut File, buf: &[u8]) -> Result<usize, Error> {
    let ops = file.fops;
    ops.write(file, buf)
}

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;
    use alloc::collections::BTreeMap;
    use alloc::string::String;
    use core::any::Any;

    use crate::file::{Error, File, FileOps, Inode};
    use super::*;

    // ---- Mock filesystem ----

    struct MockInode {
        children: BTreeMap<&'static str, &'static MockInode>,
        data: &'static [u8],
        rdev: Option<(u32, u32)>,
    }

    impl MockInode {
        fn dir(children: &[(&'static str, &'static MockInode)]) -> &'static Self {
            let mut map = BTreeMap::new();
            for &(name, inode) in children {
                map.insert(name, inode);
            }
            Box::leak(Box::new(MockInode { children: map, data: b"", rdev: None }))
        }

        fn file(data: &'static [u8]) -> &'static Self {
            Box::leak(Box::new(MockInode { children: BTreeMap::new(), data, rdev: None }))
        }

        fn chardev(major: u32, minor: u32) -> &'static Self {
            Box::leak(Box::new(MockInode { children: BTreeMap::new(), data: b"", rdev: Some((major, minor)) }))
        }
    }

    static MOCK_OPS: MockFileOps = MockFileOps;

    impl Inode for MockInode {
        fn as_any(&self) -> &dyn Any { self }
        fn file_type(&self) -> crate::file::FileType {
            if self.rdev.is_some() {
                crate::file::FileType::CharDevice
            } else if !self.children.is_empty() || self.data.is_empty() {
                crate::file::FileType::Directory
            } else {
                crate::file::FileType::RegularFile
            }
        }
        fn len(&self) -> usize { self.data.len() }
        fn file_ops(&self) -> &'static dyn FileOps { &MOCK_OPS }
        fn rdev(&self) -> Option<(u32, u32)> { self.rdev }

        fn lookup(&self, name: &str) -> Result<&'static dyn Inode, Error> {
            if self.children.is_empty() && self.data.len() > 0 {
                return Err(Error::NotADirectory);
            }
            self.children
                .get(name)
                .map(|&inode| inode as &'static dyn Inode)
                .ok_or(Error::NotFound)
        }
    }

    struct MockFileOps;

    impl FileOps for MockFileOps {
        fn read(&self, file: &mut File, buf: &mut [u8]) -> Result<usize, Error> {
            let mock = file.inode.as_any().downcast_ref::<MockInode>().unwrap();
            let remaining = &mock.data[file.offset..];
            // Return at most 3 bytes per read to exercise chunked reads
            let len = remaining.len().min(buf.len()).min(3);
            buf[..len].copy_from_slice(&remaining[..len]);
            file.offset += len;
            Ok(len)
        }
    }

    struct MockSuperBlock {
        root: &'static MockInode,
    }

    impl SuperBlock for MockSuperBlock {
        fn root_inode(&self) -> &'static dyn Inode { self.root }
        fn fs_type(&self) -> &'static str { "mock" }
    }

    fn setup(root: &'static MockInode) {
        let sb: &'static dyn SuperBlock = Box::leak(Box::new(MockSuperBlock { root }));
        init(sb);
    }

    // ---- vfs_open tests ----

    #[test_case]
    fn test_open_root() {
        println!("Testing vfs_open on root path...");
        setup(MockInode::dir(&[("a.txt", MockInode::file(b"hello"))]));

        let file = vfs_open("/").unwrap();
        assert_eq!(file.inode.file_type(), crate::file::FileType::Directory);
    }

    #[test_case]
    fn test_open_nested_path() {
        println!("Testing vfs_open with multi-component path...");
        let leaf = MockInode::file(b"data");
        let sub = MockInode::dir(&[("leaf", leaf)]);
        setup(MockInode::dir(&[("sub", sub)]));

        let file = vfs_open("/sub/leaf").unwrap();
        assert_eq!(file.inode.len(), 4);
    }

    #[test_case]
    fn test_open_not_found() {
        println!("Testing vfs_open on non-existent name...");
        setup(MockInode::dir(&[("a.txt", MockInode::file(b"x"))]));

        let result = vfs_open("/nonexistent");
        assert!(matches!(result, Err(Error::NotFound)));
    }

    #[test_case]
    fn test_open_through_file() {
        println!("Testing vfs_open traversing through a file...");
        setup(MockInode::dir(&[("a.txt", MockInode::file(b"x"))]));

        let result = vfs_open("/a.txt/child");
        assert!(matches!(result, Err(Error::NotADirectory)));
    }

    #[test_case]
    fn test_open_chardev() {
        println!("Testing vfs_open calls chrdev_open for character device...");
        setup(MockInode::dir(&[
            ("dev", MockInode::dir(&[
                ("console", MockInode::chardev(255, 255)),
            ])),
        ]));

        // vfs_open should detect file_type() == CharDevice and call chrdev_open
        // chrdev_open has no device registered for (255,255), so it returns NotFound
        let result = vfs_open("/dev/console");
        assert!(matches!(result, Err(Error::NotFound)));
    }

    // ---- vfs_read_exact tests ----

    #[test_case]
    fn test_read_exact_success() {
        println!("Testing vfs_read_exact with exact-size buffer...");
        setup(MockInode::dir(&[("f", MockInode::file(b"hello"))]));

        let mut file = vfs_open("/f").unwrap();
        let mut buf = [0u8; 5];
        vfs_read_exact(&mut file, &mut buf).unwrap();
        assert_eq!(&buf, b"hello");
    }

    #[test_case]
    fn test_read_exact_short_file() {
        println!("Testing vfs_read_exact with buffer larger than file...");
        setup(MockInode::dir(&[("f", MockInode::file(b"hi"))]));

        let mut file = vfs_open("/f").unwrap();
        let mut buf = [0u8; 10];
        let result = vfs_read_exact(&mut file, &mut buf);
        assert!(matches!(result, Err(Error::UnexpectedEof)));
    }

    #[test_case]
    fn test_read_exact_chunked() {
        println!("Testing vfs_read_exact accumulates across chunks...");
        // MockFileOps returns at most 3 bytes per read
        setup(MockInode::dir(&[("f", MockInode::file(b"abcdefgh"))]));

        let mut file = vfs_open("/f").unwrap();
        let mut buf = [0u8; 8];
        vfs_read_exact(&mut file, &mut buf).unwrap();
        assert_eq!(&buf, b"abcdefgh");
    }

    // ---- vfs_read_to_string tests ----

    #[test_case]
    fn test_read_to_string_valid_utf8() {
        println!("Testing vfs_read_to_string with valid UTF-8...");
        setup(MockInode::dir(&[("f", MockInode::file(b"hello"))]));

        let mut file = vfs_open("/f").unwrap();
        let mut out = String::new();
        vfs_read_to_string(&mut file, &mut out).unwrap();
        assert_eq!(out, "hello");
    }

    #[test_case]
    fn test_read_to_string_invalid_utf8() {
        println!("Testing vfs_read_to_string with invalid UTF-8...");
        setup(MockInode::dir(&[("f", MockInode::file(b"\xff\xfe"))]));

        let mut file = vfs_open("/f").unwrap();
        let mut out = String::new();
        let result = vfs_read_to_string(&mut file, &mut out);
        assert!(matches!(result, Err(Error::InvalidUtf8)));
    }
}
