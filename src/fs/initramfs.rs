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
pub fn new() -> Box<dyn SuperBlock> {
    let initrd_start = dtb::INITRD_START.load(Ordering::Relaxed);
    let initrd_len = dtb::INITRD_END.load(Ordering::Relaxed) - initrd_start;
    // SAFETY: DTB-provided addresses are valid for the initramfs lifetime; len is derived from start/end.
    let cpio = unsafe { slice::from_raw_parts(initrd_start as *const u8, initrd_len) };

    let ramfs = Ramfs::new();
    unpack_cpio(&ramfs, cpio);

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
        } else {
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
    use super::*;
    use crate::file::FileType;
    use crate::fs::ramfs::Ramfs;
    use crate::vfs;

    static INITRAMFS: &[u8] = include_bytes!(env!("INITRAMFS_CPIO"));

    fn setup_ramfs() {
        let ramfs = Ramfs::new();
        unpack_cpio(&ramfs, INITRAMFS);
        vfs::init(ramfs.superblock());
    }

    #[test_case]
    fn test_initramfs_magic() {
        println!("Testing CPIO file magic...");
        let magic = &INITRAMFS[..CPIO_MAGIC_LEN];
        assert_eq!(magic, CPIO_MAGIC);
    }

    #[test_case]
    fn test_cpio_single_file() {
        println!("Testing CPIO single file parsing...");
        setup_ramfs();
        let mut file = vfs::vfs_open("/test/hello.txt").unwrap();
        let mut buf = [0u8; 6];
        assert_eq!(vfs::vfs_read(&mut file, &mut buf).unwrap(), 6);
        assert_eq!(&buf, b"Hello!");
    }

    #[test_case]
    fn test_cpio_multiple_files() {
        println!("Testing CPIO multiple files parsing...");
        setup_ramfs();
        let entries = vfs::vfs_readdir("/test").unwrap();
        assert_eq!(entries.len(), 5);
    }

    #[test_case]
    fn test_cpio_nested_path() {
        println!("Testing CPIO nested path parsing...");
        setup_ramfs();
        let entries = vfs::vfs_readdir("/test").unwrap();
        let sub = entries.iter().find(|e| e.name == "sub").unwrap();
        assert_eq!(sub.file_type, FileType::Directory);
        let mut file = vfs::vfs_open("/test/sub/nested.txt").unwrap();
        let mut buf = [0u8; 15];
        assert_eq!(vfs::vfs_read(&mut file, &mut buf).unwrap(), 15);
        assert_eq!(&buf, b"Nested content.");
    }

    #[test_case]
    fn test_cpio_empty_directory() {
        println!("Testing CPIO empty directory...");
        setup_ramfs();
        let entries = vfs::vfs_readdir("/test/emptydir").unwrap();
        assert_eq!(entries.len(), 0);
    }

    #[test_case]
    fn test_cpio_empty_file() {
        println!("Testing CPIO empty file is present...");
        setup_ramfs();
        let inode = vfs::vfs_lookup("/test/empty.txt").unwrap();
        assert_eq!(inode.file_type, FileType::RegularFile);
    }

    #[test_case]
    fn test_cpio_chardev() {
        println!("Testing CPIO character device parsing...");
        setup_ramfs();
        let inode = vfs::vfs_lookup("/test/dev/tconsole").unwrap();
        assert_eq!(inode.file_type, FileType::CharDevice);
        assert_eq!(inode.rdev, Some((5, 1)));
    }

    #[test_case]
    fn test_cpio_blockdev() {
        println!("Testing CPIO block device parsing...");
        setup_ramfs();
        let inode = vfs::vfs_lookup("/test/dev/tsda").unwrap();
        assert_eq!(inode.file_type, FileType::BlockDevice);
        assert_eq!(inode.rdev, Some((8, 0)));
    }
}
