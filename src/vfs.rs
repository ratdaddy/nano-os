//! Virtual File System operations.
//!
//! Provides the kernel's file operation API. The VFS caches a root inode
//! and uses inode operations for path traversal.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ptr::{addr_of, addr_of_mut};
use core::str::from_utf8;

use crate::dev;
use crate::file::{DirEntry, Error, File, FileType, Inode, SeekFrom, SuperBlock, inode_id};

// =============================================================================
// SuperBlock and Mount Table
// =============================================================================

/// A single mount record.
struct Mount {
    mountpoint_inode: Option<Arc<Inode>>,
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
        let mounts = addr_of_mut!(MOUNTS);
        *mounts = Some(Vec::new());
        (*mounts).as_mut().unwrap().push(Mount { mountpoint_inode: None, mountpoint: "/", sb });
    }
}

/// Return mount info for all mounted filesystems.
pub fn mounts() -> Vec<MountInfo> {
    unsafe {
        let mounts = addr_of!(MOUNTS);
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
///
/// `source` is the device to mount (e.g. `Some("/dev/sda2")`) for block-based
/// filesystems, or `None` for virtual filesystems (procfs, ramfs).
pub fn vfs_mount_at(source: Option<&str>, path: &'static str, fs_name: &str) -> Result<(), Error> {
    let inode = vfs_lookup(path)?;
    let fs = find_filesystem(fs_name).ok_or(Error::NotFound)?;
    let sb = fs.mount(source)?;
    unsafe {
        let mounts = addr_of_mut!(MOUNTS);
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
    /// Returns true if this filesystem does not require a block device.
    /// Matches the Linux `nodev` flag shown in /proc/filesystems.
    fn nodev(&self) -> bool { false }
    /// Mount the filesystem and return a SuperBlock.
    ///
    /// `source` is the device path for block-based filesystems (e.g. "/dev/sda2").
    /// Virtual filesystems (procfs, ramfs) ignore it and accept `None`.
    fn mount(&self, source: Option<&str>) -> Result<&'static dyn SuperBlock, Error>;
}

static mut FILESYSTEMS: Option<Vec<&'static dyn FileSystem>> = None;

/// Register a filesystem driver.
pub fn register_filesystem(fs: &'static dyn FileSystem) {
    unsafe {
        let fss = addr_of_mut!(FILESYSTEMS);
        if (*fss).is_none() {
            *fss = Some(Vec::new());
        }
        (*fss).as_mut().unwrap().push(fs);
    }
}

/// Look up a registered filesystem by name.
pub fn find_filesystem(name: &str) -> Option<&'static dyn FileSystem> {
    unsafe {
        let fss = addr_of!(FILESYSTEMS);
        (*fss).as_ref()
            .and_then(|fss| fss.iter().find(|fs| fs.name() == name))
            .copied()
    }
}

/// Return all registered filesystem drivers.
pub fn filesystems() -> Vec<&'static dyn FileSystem> {
    unsafe {
        let fss = addr_of!(FILESYSTEMS);
        (*fss).as_ref().map_or_else(Vec::new, |fss| fss.clone())
    }
}

// =============================================================================
// Path Lookup and File Operations
// =============================================================================

/// Return the root inode from mount 0.
fn root_inode() -> Result<Arc<Inode>, Error> {
    unsafe {
        let mounts = addr_of!(MOUNTS);
        Ok((*mounts).as_ref().ok_or(Error::InvalidInput)?[0].sb.root_inode())
    }
}

/// Check if an inode is a mountpoint; if so, return the mounted root inode.
fn cross_mount(inode: Arc<Inode>) -> Arc<Inode> {
    unsafe {
        let mounts = addr_of!(MOUNTS);
        if let Some(mounts) = (*mounts).as_ref() {
            for mount in mounts {
                if let Some(ref mp) = mount.mountpoint_inode {
                    if inode_id(mp) == inode_id(&inode) {
                        return mount.sb.root_inode();
                    }
                }
            }
        }
    }
    inode
}

/// Look up an inode by path without opening it.
pub fn vfs_lookup(path: &str) -> Result<Arc<Inode>, Error> {
    let mut inode = root_inode()?;
    for component in path.split('/').filter(|s| !s.is_empty()) {
        inode = inode.iops.lookup(&inode, component)?;
        inode = cross_mount(inode);
    }
    Ok(inode)
}

/// Open a file by path.
pub fn vfs_open(path: &str) -> Result<File, Error> {
    let inode = vfs_lookup(path)?;
    if inode.file_type == FileType::CharDevice {
        return dev::chrdev_open(inode);
    }
    let fops = inode.fops;
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
pub fn vfs_read_to_string(file: &mut File, out: &mut String) -> Result<(), Error> {
    let mut buf = [0u8; 256];
    loop {
        let len = vfs_read(file, &mut buf)?;
        if len == 0 {
            break;
        }
        let s = from_utf8(&buf[..len]).map_err(|_| Error::InvalidUtf8)?;
        out.push_str(s);
    }
    Ok(())
}

/// Write a buffer to a file.
/// Returns the number of bytes written.
pub fn vfs_write(file: &mut File, buf: &[u8]) -> Result<usize, Error> {
    let ops = file.fops;
    ops.write(file, buf)
}

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;
    use alloc::collections::BTreeMap;
    use alloc::string::String;
    use alloc::sync::Arc;

    use crate::file::{Error, File, FileOps, FileType, Inode, InodeOps};
    use super::*;

    // ---- Mock filesystem ----

    struct MockDirData {
        children: BTreeMap<&'static str, Arc<Inode>>,
    }

    struct MockInodeOps;
    struct MockFileOps;

    static MOCK_INODE_OPS: MockInodeOps = MockInodeOps;
    static MOCK_FILE_OPS: MockFileOps = MockFileOps;

    impl InodeOps for MockInodeOps {
        fn lookup(&self, inode: &Arc<Inode>, name: &str) -> Result<Arc<Inode>, Error> {
            if inode.file_type != FileType::Directory {
                return Err(Error::NotADirectory);
            }
            let data = inode.fs_data.downcast_ref::<MockDirData>().unwrap();
            data.children.get(name).cloned().ok_or(Error::NotFound)
        }
    }

    impl FileOps for MockFileOps {
        fn read(&self, file: &mut File, buf: &mut [u8]) -> Result<usize, Error> {
            let data = file.inode.fs_data.downcast_ref::<&'static [u8]>().unwrap();
            let remaining = &data[file.offset..];
            // Return at most 3 bytes per read to exercise chunked reads
            let len = remaining.len().min(buf.len()).min(3);
            buf[..len].copy_from_slice(&remaining[..len]);
            file.offset += len;
            Ok(len)
        }
    }

    fn mock_file(data: &'static [u8]) -> Arc<Inode> {
        Arc::new(Inode {
            ino: 0,
            file_type: FileType::RegularFile,
            len: data.len(),
            iops: &MOCK_INODE_OPS,
            fops: &MOCK_FILE_OPS,
            sb: None,
            rdev: None,
            fs_data: Box::new(data),
        })
    }

    fn mock_dir(children: &[(&'static str, Arc<Inode>)]) -> Arc<Inode> {
        let mut map = BTreeMap::new();
        for (name, inode) in children {
            map.insert(*name, Arc::clone(inode));
        }
        Arc::new(Inode {
            ino: 0,
            file_type: FileType::Directory,
            len: 0,
            iops: &MOCK_INODE_OPS,
            fops: &MOCK_FILE_OPS,
            sb: None,
            rdev: None,
            fs_data: Box::new(MockDirData { children: map }),
        })
    }

    fn mock_chardev(major: u32, minor: u32) -> Arc<Inode> {
        Arc::new(Inode {
            ino: 0,
            file_type: FileType::CharDevice,
            len: 0,
            iops: &MOCK_INODE_OPS,
            fops: &MOCK_FILE_OPS,
            sb: None,
            rdev: Some((major, minor)),
            fs_data: Box::new(()),
        })
    }

    struct MockSuperBlock {
        root: Arc<Inode>,
    }

    impl SuperBlock for MockSuperBlock {
        fn root_inode(&self) -> Arc<Inode> { Arc::clone(&self.root) }
        fn fs_type(&self) -> &'static str { "mock" }
    }

    fn setup(root: Arc<Inode>) {
        let sb: &'static dyn SuperBlock = Box::leak(Box::new(MockSuperBlock { root }));
        init(sb);
    }

    /// Add a mount over the given inode, pointing to a second mock filesystem.
    fn mount_over(mountpoint: Arc<Inode>, root: Arc<Inode>) {
        let sb: &'static dyn SuperBlock = Box::leak(Box::new(MockSuperBlock { root }));
        unsafe {
            let mounts = addr_of_mut!(MOUNTS);
            (*mounts).as_mut().unwrap().push(Mount {
                mountpoint_inode: Some(mountpoint),
                mountpoint: "",
                sb,
            });
        }
    }

    // ---- cross_mount tests ----

    #[test_case]
    fn test_lookup_crosses_mount() {
        println!("Testing vfs_lookup crosses mountpoint...");
        let mnt = mock_dir(&[]);
        setup(mock_dir(&[("mnt", Arc::clone(&mnt))]));

        let mounted_root = mock_dir(&[("data.txt", mock_file(b"mounted"))]);
        mount_over(Arc::clone(&mnt), Arc::clone(&mounted_root));

        let mut file = vfs_open("/mnt/data.txt").unwrap();
        let mut buf = [0u8; 7];
        vfs_read_exact(&mut file, &mut buf).unwrap();
        assert_eq!(&buf, b"mounted");
    }

    #[test_case]
    fn test_lookup_mountpoint_returns_mounted_root() {
        println!("Testing vfs_lookup on mountpoint returns mounted root...");
        let mnt = mock_dir(&[]);
        setup(mock_dir(&[("mnt", Arc::clone(&mnt))]));

        let mounted_root = mock_dir(&[("inner.txt", mock_file(b"x"))]);
        mount_over(Arc::clone(&mnt), Arc::clone(&mounted_root));

        let inode = vfs_lookup("/mnt").unwrap();
        assert_eq!(inode_id(&inode), inode_id(&mounted_root));
    }

    #[test_case]
    fn test_lookup_not_found_in_mounted_fs() {
        println!("Testing vfs_lookup not found in mounted filesystem...");
        let mnt = mock_dir(&[]);
        setup(mock_dir(&[("mnt", Arc::clone(&mnt))]));

        let mounted_root = mock_dir(&[("exists.txt", mock_file(b"y"))]);
        mount_over(Arc::clone(&mnt), Arc::clone(&mounted_root));

        let result = vfs_open("/mnt/nonexistent");
        assert!(matches!(result, Err(Error::NotFound)));
    }

    // ---- vfs_open tests ----

    #[test_case]
    fn test_open_root() {
        println!("Testing vfs_open on root path...");
        setup(mock_dir(&[("a.txt", mock_file(b"hello"))]));

        let file = vfs_open("/").unwrap();
        assert_eq!(file.inode.file_type, FileType::Directory);
    }

    #[test_case]
    fn test_open_nested_path() {
        println!("Testing vfs_open with multi-component path...");
        let leaf = mock_file(b"data");
        let sub = mock_dir(&[("leaf", Arc::clone(&leaf))]);
        setup(mock_dir(&[("sub", Arc::clone(&sub))]));

        let file = vfs_open("/sub/leaf").unwrap();
        assert_eq!(file.inode.len, 4);
    }

    #[test_case]
    fn test_open_not_found() {
        println!("Testing vfs_open on non-existent name...");
        setup(mock_dir(&[("a.txt", mock_file(b"x"))]));

        let result = vfs_open("/nonexistent");
        assert!(matches!(result, Err(Error::NotFound)));
    }

    #[test_case]
    fn test_open_through_file() {
        println!("Testing vfs_open traversing through a file...");
        setup(mock_dir(&[("a.txt", mock_file(b"x"))]));

        let result = vfs_open("/a.txt/child");
        assert!(matches!(result, Err(Error::NotADirectory)));
    }

    #[test_case]
    fn test_open_chardev() {
        println!("Testing vfs_open calls chrdev_open for character device...");
        setup(mock_dir(&[
            ("dev", mock_dir(&[
                ("console", mock_chardev(255, 255)),
            ])),
        ]));

        // vfs_open should detect file_type == CharDevice and call chrdev_open
        // chrdev_open has no device registered for (255,255), so it returns NotFound
        let result = vfs_open("/dev/console");
        assert!(matches!(result, Err(Error::NotFound)));
    }

    // ---- vfs_read_exact tests ----

    #[test_case]
    fn test_read_exact_success() {
        println!("Testing vfs_read_exact with exact-size buffer...");
        setup(mock_dir(&[("f", mock_file(b"hello"))]));

        let mut file = vfs_open("/f").unwrap();
        let mut buf = [0u8; 5];
        vfs_read_exact(&mut file, &mut buf).unwrap();
        assert_eq!(&buf, b"hello");
    }

    #[test_case]
    fn test_read_exact_short_file() {
        println!("Testing vfs_read_exact with buffer larger than file...");
        setup(mock_dir(&[("f", mock_file(b"hi"))]));

        let mut file = vfs_open("/f").unwrap();
        let mut buf = [0u8; 10];
        let result = vfs_read_exact(&mut file, &mut buf);
        assert!(matches!(result, Err(Error::UnexpectedEof)));
    }

    #[test_case]
    fn test_read_exact_chunked() {
        println!("Testing vfs_read_exact accumulates across chunks...");
        // MockFileOps returns at most 3 bytes per read
        setup(mock_dir(&[("f", mock_file(b"abcdefgh"))]));

        let mut file = vfs_open("/f").unwrap();
        let mut buf = [0u8; 8];
        vfs_read_exact(&mut file, &mut buf).unwrap();
        assert_eq!(&buf, b"abcdefgh");
    }

    // ---- vfs_read_to_string tests ----

    #[test_case]
    fn test_read_to_string_valid_utf8() {
        println!("Testing vfs_read_to_string with valid UTF-8...");
        setup(mock_dir(&[("f", mock_file(b"hello"))]));

        let mut file = vfs_open("/f").unwrap();
        let mut out = String::new();
        vfs_read_to_string(&mut file, &mut out).unwrap();
        assert_eq!(out, "hello");
    }

    #[test_case]
    fn test_read_to_string_invalid_utf8() {
        println!("Testing vfs_read_to_string with invalid UTF-8...");
        setup(mock_dir(&[("f", mock_file(b"\xff\xfe"))]));

        let mut file = vfs_open("/f").unwrap();
        let mut out = String::new();
        let result = vfs_read_to_string(&mut file, &mut out);
        assert!(matches!(result, Err(Error::InvalidUtf8)));
    }
}
