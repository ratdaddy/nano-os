//! Initramfs support - unpacks CPIO archives into ramfs.

use alloc::boxed::Box;
use core::slice;
use core::sync::atomic::Ordering;

use crate::dtb;
use crate::fs::ramfs::Ramfs;
use crate::file::{SuperBlock, S_IFMT, S_IFDIR, S_IFCHR, S_IFBLK};

// CPIO "newc" format layout
const CPIO_MAGIC: &[u8] = b"070701";
const CPIO_MAGIC_LEN: usize = 6;
const CPIO_HEADER_SIZE: usize = 110;
const CPIO_ALIGN: usize = 4;
const CPIO_FIELD_WIDTH: usize = 8; // each header field is 8 hex characters

// CPIO header field offsets (byte position within the 110-byte header)
const CPIO_MODE_OFFSET: usize = 14;
const CPIO_FILESIZE_OFFSET: usize = 54;
const CPIO_RDEVMAJOR_OFFSET: usize = 78;
const CPIO_RDEVMINOR_OFFSET: usize = 86;
const CPIO_NAMESIZE_OFFSET: usize = 94;

/// Create a ramfs populated from the DTB-specified initramfs location.
/// Returns a SuperBlock for registration with VFS.
/// Must be called after dtb::parse_dtb().
pub fn new() -> &'static dyn SuperBlock {
    let initrd_start = dtb::INITRD_START.load(Ordering::Relaxed);
    let initrd_len = dtb::INITRD_END.load(Ordering::Relaxed) - initrd_start;
    // SAFETY: DTB-provided addresses are valid for the initramfs lifetime; len is derived from start/end.
    let cpio = unsafe { slice::from_raw_parts(initrd_start as *const u8, initrd_len) };

    // Create ramfs and populate from CPIO
    let ramfs = Box::leak(Box::new(Ramfs::new()));
    unpack_cpio(ramfs, cpio);

    ramfs.superblock()
}

/// Unpack a CPIO "newc" archive into a ramfs instance.
pub(crate) fn unpack_cpio(ramfs: &Ramfs, cpio: &'static [u8]) {
    let mut pos = 0;

    while pos + CPIO_HEADER_SIZE <= cpio.len() {
        let hdr = &cpio[pos..];
        if &hdr[..CPIO_MAGIC_LEN] != CPIO_MAGIC {
            break;
        }

        let mode = parse_hex(&hdr[CPIO_MODE_OFFSET..CPIO_MODE_OFFSET + CPIO_FIELD_WIDTH]) as u16;
        let filesize = parse_hex(&hdr[CPIO_FILESIZE_OFFSET..CPIO_FILESIZE_OFFSET + CPIO_FIELD_WIDTH]);
        let namesize = parse_hex(&hdr[CPIO_NAMESIZE_OFFSET..CPIO_NAMESIZE_OFFSET + CPIO_FIELD_WIDTH]);

        let name_start = pos + CPIO_HEADER_SIZE;
        let name_end = name_start + namesize;
        let filename = match core::str::from_utf8(&cpio[name_start..name_end - 1]) {
            Ok(s) => s,
            Err(_) => break,
        };

        if filename == "TRAILER!!!" {
            break;
        }

        let data_start = align_up(name_end, CPIO_ALIGN);
        let data_end = data_start + filesize;

        let fmt = mode & S_IFMT;
        if fmt == S_IFDIR {
            let _ = ramfs.insert_dir(filename);
        } else if fmt == S_IFCHR {
            let rdevmajor = parse_hex(&hdr[CPIO_RDEVMAJOR_OFFSET..CPIO_RDEVMAJOR_OFFSET + CPIO_FIELD_WIDTH]) as u32;
            let rdevminor = parse_hex(&hdr[CPIO_RDEVMINOR_OFFSET..CPIO_RDEVMINOR_OFFSET + CPIO_FIELD_WIDTH]) as u32;
            let _ = ramfs.insert_chardev(filename, rdevmajor, rdevminor);
        } else if fmt == S_IFBLK {
            let rdevmajor = parse_hex(&hdr[CPIO_RDEVMAJOR_OFFSET..CPIO_RDEVMAJOR_OFFSET + CPIO_FIELD_WIDTH]) as u32;
            let rdevminor = parse_hex(&hdr[CPIO_RDEVMINOR_OFFSET..CPIO_RDEVMINOR_OFFSET + CPIO_FIELD_WIDTH]) as u32;
            let _ = ramfs.insert_blockdev(filename, rdevmajor, rdevminor);
        } else if filesize > 0 {
            let _ = ramfs.insert_file(filename, &cpio[data_start..data_end]);
        }

        pos = align_up(data_end, CPIO_ALIGN);
    }
}

fn align_up(x: usize, align: usize) -> usize {
    (x + align - 1) & !(align - 1)
}

fn parse_hex(bytes: &[u8]) -> usize {
    usize::from_str_radix(core::str::from_utf8(bytes).unwrap_or("0"), 16).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;
    use alloc::format;
    use alloc::vec::Vec;
    use core::slice;
    use super::*;

    const CPIO_TRAILER: &[u8] = b"TRAILER!!!\0";
    use crate::file::{FileType, SuperBlock};
    use crate::fs::ramfs::{Ramfs, RamfsSuperBlock};
    use crate::test;
    use crate::vfs;

    /// Build a CPIO "newc" archive from a list of (name, data, mode, rdevmajor, rdevminor) entries.
    fn make_cpio(entries: &[(&str, &[u8], u16, u32, u32)]) -> &'static [u8] {
        let mut buf = Vec::new();

        for &(name, data, mode, rdevmajor, rdevminor) in entries {
            let name_cstr = format!("{}\0", name);
            let namesize = name_cstr.len();
            let filesize = data.len();

            // Write CPIO "newc" header
            buf.extend_from_slice(CPIO_MAGIC);
            buf.extend_from_slice(&[b'0'; CPIO_HEADER_SIZE - CPIO_MAGIC_LEN]);
            let hdr_start = buf.len() - CPIO_HEADER_SIZE;
            let o = CPIO_FIELD_WIDTH;
            buf[hdr_start + CPIO_MODE_OFFSET..hdr_start + CPIO_MODE_OFFSET + o]
                .copy_from_slice(format!("{:08X}", mode).as_bytes());
            buf[hdr_start + CPIO_FILESIZE_OFFSET..hdr_start + CPIO_FILESIZE_OFFSET + o]
                .copy_from_slice(format!("{:08X}", filesize).as_bytes());
            buf[hdr_start + CPIO_RDEVMAJOR_OFFSET..hdr_start + CPIO_RDEVMAJOR_OFFSET + o]
                .copy_from_slice(format!("{:08X}", rdevmajor).as_bytes());
            buf[hdr_start + CPIO_RDEVMINOR_OFFSET..hdr_start + CPIO_RDEVMINOR_OFFSET + o]
                .copy_from_slice(format!("{:08X}", rdevminor).as_bytes());
            buf[hdr_start + CPIO_NAMESIZE_OFFSET..hdr_start + CPIO_NAMESIZE_OFFSET + o]
                .copy_from_slice(format!("{:08X}", namesize).as_bytes());

            buf.extend_from_slice(name_cstr.as_bytes());
            while buf.len() % CPIO_ALIGN != 0 {
                buf.push(0);
            }

            buf.extend_from_slice(data);
            while buf.len() % CPIO_ALIGN != 0 {
                buf.push(0);
            }
        }

        // Append TRAILER!!! entry
        buf.extend_from_slice(CPIO_MAGIC);
        buf.extend_from_slice(&[b'0'; CPIO_HEADER_SIZE - CPIO_MAGIC_LEN]);
        let trailer_start = buf.len() - CPIO_HEADER_SIZE;
        let o = CPIO_FIELD_WIDTH;
        buf[trailer_start + CPIO_NAMESIZE_OFFSET..trailer_start + CPIO_NAMESIZE_OFFSET + o]
            .copy_from_slice(format!("{:08X}", CPIO_TRAILER.len()).as_bytes());
        buf.extend_from_slice(CPIO_TRAILER);
        while buf.len() % CPIO_ALIGN != 0 {
            buf.push(0);
        }

        let boxed = buf.into_boxed_slice();
        let len = boxed.len();
        let ptr = Box::into_raw(boxed) as *mut u8;
        test::register_leak(ptr, len);
        // SAFETY: ptr was produced by Box::into_raw with matching len.
        unsafe { slice::from_raw_parts(ptr, len) }
    }

    /// Helper to set up a test ramfs from CPIO and register with VFS.
    fn setup_test_ramfs(cpio: &'static [u8]) -> &'static Ramfs {
        let ramfs = test::register_typed_leak(Box::new(Ramfs::new()));
        unpack_cpio(ramfs, cpio);
        let sb: &'static dyn SuperBlock = test::register_typed_leak::<RamfsSuperBlock>(ramfs.superblock_for_test());
        vfs::init(sb);
        ramfs
    }

    #[test_case]
    fn test_cpio_single_file() {
        println!("Testing CPIO single file parsing...");

        let cpio = make_cpio(&[("hello.txt", b"Hello!", 0o100644, 0, 0)]);
        setup_test_ramfs(cpio);

        let mut file = vfs::vfs_open("/hello.txt").unwrap();
        let mut buf = [0u8; 6];
        assert_eq!(vfs::vfs_read(&mut file, &mut buf).unwrap(), 6);
        assert_eq!(&buf, b"Hello!");
    }

    #[test_case]
    fn test_cpio_multiple_files() {
        println!("Testing CPIO multiple files parsing...");

        let cpio = make_cpio(&[
            ("one.txt", b"first", 0o100644, 0, 0),
            ("two.txt", b"second", 0o100644, 0, 0),
        ]);
        setup_test_ramfs(cpio);

        let entries = vfs::vfs_readdir("/").unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test_case]
    fn test_cpio_nested_path() {
        println!("Testing CPIO nested path parsing...");

        let cpio = make_cpio(&[("etc/motd", b"Welcome to nano-os!", 0o100644, 0, 0)]);
        setup_test_ramfs(cpio);

        // Should create /etc directory and /etc/motd file
        let etc_entries = vfs::vfs_readdir("/etc").unwrap();
        assert_eq!(etc_entries.len(), 1);
        assert_eq!(etc_entries[0].name, "motd");

        let mut file = vfs::vfs_open("/etc/motd").unwrap();
        let mut buf = [0u8; 19];
        assert_eq!(vfs::vfs_read(&mut file, &mut buf).unwrap(), 19);
        assert_eq!(&buf, b"Welcome to nano-os!");
    }

    #[test_case]
    fn test_cpio_creates_directories() {
        println!("Testing CPIO creates directory entries...");

        // Mode 0o040755 = directory
        let cpio = make_cpio(&[
            ("mydir", b"", 0o040755, 0, 0),
            ("mydir/file.txt", b"content", 0o100644, 0, 0),
        ]);
        setup_test_ramfs(cpio);

        let entries = vfs::vfs_readdir("/mydir").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "file.txt");
    }

    #[test_case]
    fn test_cpio_empty_directory() {
        println!("Testing CPIO empty directory...");

        let cpio = make_cpio(&[
            ("mnt", b"", 0o040755, 0, 0),
        ]);
        setup_test_ramfs(cpio);

        let entries = vfs::vfs_readdir("/").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "mnt");
        assert_eq!(entries[0].file_type, FileType::Directory);

        let mnt_entries = vfs::vfs_readdir("/mnt").unwrap();
        assert_eq!(mnt_entries.len(), 0);
    }

    #[test_case]
    fn test_cpio_empty_file_skipped() {
        println!("Testing CPIO empty file is skipped...");

        let cpio = make_cpio(&[
            ("empty.txt", b"", 0o100644, 0, 0),
            ("nonempty.txt", b"data", 0o100644, 0, 0),
        ]);
        setup_test_ramfs(cpio);

        // Empty file should be skipped, only nonempty.txt present
        let entries = vfs::vfs_readdir("/").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "nonempty.txt");
    }

    #[test_case]
    fn test_cpio_chardev() {
        println!("Testing CPIO character device parsing...");

        let cpio = make_cpio(&[
            ("dev", b"", 0o040755, 0, 0),
            ("dev/console", b"", 0o020600, 5, 1),
        ]);
        setup_test_ramfs(cpio);

        let entries = vfs::vfs_readdir("/dev").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "console");
        assert_eq!(entries[0].file_type, FileType::CharDevice);

        let inode = vfs::vfs_lookup("/dev/console").unwrap();
        assert_eq!(inode.rdev, Some((5, 1)));
    }

    #[test_case]
    fn test_cpio_blockdev() {
        println!("Testing CPIO block device parsing...");

        let cpio = make_cpio(&[
            ("dev", b"", 0o040755, 0, 0),
            ("dev/sda", b"", 0o060600, 8, 0),
        ]);
        setup_test_ramfs(cpio);

        let entries = vfs::vfs_readdir("/dev").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "sda");
        assert_eq!(entries[0].file_type, FileType::BlockDevice);

        let inode = vfs::vfs_lookup("/dev/sda").unwrap();
        assert_eq!(inode.rdev, Some((8, 0)));
    }
}
