//! Character device registry and open logic.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use core::fmt;
use core::ptr::{addr_of, addr_of_mut};

use crate::file::{Error, File, FileOps, Inode};

struct CharDevEntry {
    name: &'static str,
    fops: &'static dyn FileOps,
}

impl fmt::Debug for CharDevEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CharDevEntry").field("name", &self.name).finish_non_exhaustive()
    }
}

static mut CHARDEVS: Option<BTreeMap<(u32, u32), CharDevEntry>> = None;

/// Register a character device driver for the given major/minor.
pub fn chrdev_register(major: u32, minor: u32, name: &'static str, fops: &'static dyn FileOps) {
    // SAFETY: called during boot before any threads are spawned; no concurrent access to CHARDEVS.
    unsafe {
        let chardevs = addr_of_mut!(CHARDEVS);
        if (*chardevs).is_none() {
            *chardevs = Some(BTreeMap::new());
        }
        (*chardevs).as_mut().unwrap().insert((major, minor), CharDevEntry { name, fops });
    }
}

/// Call `f` for each registered character device: (major, minor, name).
pub fn chrdev_for_each(mut f: impl FnMut(u32, u32, &'static str)) {
    // SAFETY: single-HART kernel; CHARDEVS is not accessed from interrupt handlers.
    // See backlog/static_mut_to_mutex.md for the planned Mutex migration.
    unsafe {
        let chardevs = addr_of!(CHARDEVS);
        if let Some(devs) = (*chardevs).as_ref() {
            for (&(major, minor), entry) in devs {
                f(major, minor, entry.name);
            }
        }
    }
}

/// Open a character device by looking up its registered driver.
pub fn chrdev_open(inode: Arc<Inode>) -> Result<File, Error> {
    let (major, minor) = inode.rdev.ok_or(Error::InvalidInput)?;
    // SAFETY: single-HART kernel; CHARDEVS is not accessed from interrupt handlers.
    // See backlog/static_mut_to_mutex.md for the planned Mutex migration.
    let fops = unsafe {
        let chardevs = addr_of!(CHARDEVS);
        (*chardevs).as_ref()
            .and_then(|devs| devs.get(&(major, minor)))
            .map(|e| e.fops)
            .ok_or(Error::NotFound)?
    };
    Ok(File::new(fops, inode))
}

#[cfg(test)]
pub fn chrdev_clear() {
    // SAFETY: tests are single-threaded; no concurrent access to CHARDEVS.
    unsafe { *addr_of_mut!(CHARDEVS) = None; }
}

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;
    use alloc::sync::Arc;

    use crate::file::{Error, File, FileOps, FileType, Inode, InodeOps};
    use super::*;

    // ---- Mock infrastructure ----

    struct MockInodeOps;
    impl InodeOps for MockInodeOps {}
    static MOCK_INODE_OPS: MockInodeOps = MockInodeOps;

    /// The inode's own file ops — should NOT be used when chrdev_open succeeds.
    struct MockInodeFileOps;
    impl FileOps for MockInodeFileOps {
        fn read(&self, _file: &mut File, buf: &mut [u8]) -> Result<usize, Error> {
            if buf.is_empty() { return Ok(0); }
            buf[0] = b'?';
            Ok(1)
        }
    }
    static MOCK_INODE_FILE_OPS: MockInodeFileOps = MockInodeFileOps;

    /// The registered device ops — chrdev_open should use these.
    struct MockDevFileOps;
    impl FileOps for MockDevFileOps {
        fn read(&self, _file: &mut File, buf: &mut [u8]) -> Result<usize, Error> {
            if buf.is_empty() { return Ok(0); }
            buf[0] = b'!';
            Ok(1)
        }
    }
    static MOCK_DEV_OPS: MockDevFileOps = MockDevFileOps;

    fn mock_chardev_inode(major: u32, minor: u32) -> Arc<Inode> {
        Arc::new(Inode {
            ino: 0,
            file_type: FileType::CharDevice,
            len: 0,
            iops: &MOCK_INODE_OPS,
            fops: &MOCK_INODE_FILE_OPS,
            sb: None,
            rdev: Some((major, minor)),
            fs_data: Box::new(()),
        })
    }

    // ---- Tests ----

    #[test_case]
    fn test_chrdev_register_and_open() {
        println!("Testing chrdev_register and chrdev_open...");

        chrdev_register(5, 1, "mock_dev", &MOCK_DEV_OPS);

        let inode = mock_chardev_inode(5, 1);
        let mut file = chrdev_open(inode).unwrap();

        // Verify we got the registered device ops, not the inode's own ops
        let mut buf = [0u8; 1];
        let ops = file.fops;
        ops.read(&mut file, &mut buf).unwrap();
        assert_eq!(buf[0], b'!');
    }

    #[test_case]
    fn test_chrdev_open_not_registered() {
        println!("Testing chrdev_open on unregistered device...");

        let inode = mock_chardev_inode(99, 99);
        let result = chrdev_open(inode);
        assert!(matches!(result, Err(Error::NotFound)));
    }
}
