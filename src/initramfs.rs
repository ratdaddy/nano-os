//! Initramfs support - unpacks CPIO archives into ramfs.

use alloc::boxed::Box;
use core::sync::atomic::Ordering;

use crate::dtb;
use crate::file::Inode;
use crate::ramfs::Ramfs;

/// Create a ramfs populated from the DTB-specified initramfs location.
/// Returns the root inode for registration with VFS.
/// Must be called after dtb::parse_dtb().
pub fn new() -> &'static dyn Inode {
    let initrd_start = dtb::INITRD_START.load(Ordering::Relaxed);
    let initrd_len = dtb::INITRD_END.load(Ordering::Relaxed) - initrd_start;
    let cpio = unsafe { core::slice::from_raw_parts(initrd_start as *const u8, initrd_len) };

    // Create ramfs and populate from CPIO
    let ramfs = Box::leak(Box::new(Ramfs::new()));
    unpack_cpio(ramfs, cpio);

    ramfs.root()
}

/// Unpack a CPIO "newc" archive into a ramfs instance.
pub(crate) fn unpack_cpio(ramfs: &Ramfs, cpio: &'static [u8]) {
    let mut pos = 0;

    while pos + 110 <= cpio.len() {
        let hdr = &cpio[pos..];
        if &hdr[0..6] != b"070701" {
            break;
        }

        let mode = parse_hex(&hdr[14..22]) as u16;
        let filesize = parse_hex(&hdr[54..62]);
        let namesize = parse_hex(&hdr[94..102]);

        let name_start = pos + 110;
        let name_end = name_start + namesize;
        let filename = match core::str::from_utf8(&cpio[name_start..name_end - 1]) {
            Ok(s) => s,
            Err(_) => break,
        };

        if filename == "TRAILER!!!" {
            break;
        }

        let data_start = align_up(name_end, 4);
        let data_end = data_start + filesize;

        let fmt = mode & 0o170000;
        if fmt == 0o040000 {
            let _ = ramfs.insert_dir(filename);
        } else if fmt == 0o020000 {
            let rdevmajor = parse_hex(&hdr[78..86]) as u32;
            let rdevminor = parse_hex(&hdr[86..94]) as u32;
            let _ = ramfs.insert_chardev(filename, rdevmajor, rdevminor);
        } else if filesize > 0 {
            let _ = ramfs.insert_file(filename, &cpio[data_start..data_end]);
        }

        pos = align_up(data_end, 4);
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
    use super::*;
    use crate::ramfs::Ramfs;
    use crate::vfs;

    /// Build a CPIO "newc" archive from a list of (name, data, mode, rdevmajor, rdevminor) entries.
    fn make_cpio(entries: &[(&str, &[u8], u16, u32, u32)]) -> &'static [u8] {
        let mut buf = Vec::new();

        for &(name, data, mode, rdevmajor, rdevminor) in entries {
            let name_cstr = format!("{}\0", name);
            let namesize = name_cstr.len();
            let filesize = data.len();

            // Write CPIO "newc" header (110 bytes)
            buf.extend_from_slice(b"070701"); // c_magic
            buf.extend_from_slice(&[b'0'; 110 - 6]); // fill with '0's
            let hdr_start = buf.len() - 110;
            // c_mode at offset 14
            buf[hdr_start + 14..hdr_start + 22]
                .copy_from_slice(format!("{:08X}", mode).as_bytes());
            // c_filesize at offset 54
            buf[hdr_start + 54..hdr_start + 62]
                .copy_from_slice(format!("{:08X}", filesize).as_bytes());
            // c_rdevmajor at offset 78
            buf[hdr_start + 78..hdr_start + 86]
                .copy_from_slice(format!("{:08X}", rdevmajor).as_bytes());
            // c_rdevminor at offset 86
            buf[hdr_start + 86..hdr_start + 94]
                .copy_from_slice(format!("{:08X}", rdevminor).as_bytes());
            // c_namesize at offset 94
            buf[hdr_start + 94..hdr_start + 102]
                .copy_from_slice(format!("{:08X}", namesize).as_bytes());

            buf.extend_from_slice(name_cstr.as_bytes());
            while buf.len() % 4 != 0 {
                buf.push(0);
            }

            buf.extend_from_slice(data);
            while buf.len() % 4 != 0 {
                buf.push(0);
            }
        }

        // Append TRAILER!!! entry
        buf.extend_from_slice(b"070701");
        buf.extend_from_slice(&[b'0'; 110 - 6]);
        let trailer_start = buf.len() - 110;
        buf[trailer_start + 94..trailer_start + 102].copy_from_slice(b"0000000B");
        buf.extend_from_slice(b"TRAILER!!!\0");
        while buf.len() % 4 != 0 {
            buf.push(0);
        }

        Box::leak(buf.into_boxed_slice())
    }

    /// Helper to set up a test ramfs from CPIO and register with VFS.
    fn setup_test_ramfs(cpio: &'static [u8]) -> &'static Ramfs {
        let ramfs = Box::leak(Box::new(Ramfs::new()));
        unpack_cpio(ramfs, cpio);
        vfs::init(ramfs.root());
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
        assert_eq!(entries[0].file_type, crate::file::FileType::Directory);

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
        assert_eq!(entries[0].file_type, crate::file::FileType::CharDevice);

        let file = vfs::vfs_open("/dev/console").unwrap();
        assert_eq!(file.inode.unwrap().rdev(), Some((5, 1)));
    }
}
