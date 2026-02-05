#![allow(static_mut_refs)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::mem::MaybeUninit;
use core::sync::atomic::Ordering;

use crate::dtb;
use crate::file::{self, File, FileOps};

static mut FILES: MaybeUninit<Vec<FileEntry>> = MaybeUninit::uninit();

/// Initialize the initramfs by mounting from the DTB-specified location.
/// Must be called after dtb::parse_dtb() and before any ifs_open() calls.
pub fn init() {
    let initrd_start = dtb::INITRD_START.load(Ordering::Relaxed);
    let initrd_len = dtb::INITRD_END.load(Ordering::Relaxed) - initrd_start;
    let slice = unsafe { core::slice::from_raw_parts(initrd_start as *const u8, initrd_len) };
    ifs_mount(slice);
}

struct FileEntry {
    path: String,
    data: &'static [u8],
}

/// Static file operations for initramfs files.
/// Per-file state (data pointer, offset) is stored in the File struct.
pub struct IfsFileOps;

impl FileOps for IfsFileOps {
    fn read(&self, file: &mut File, buf: &mut [u8]) -> Result<usize, file::Error> {
        let data = unsafe { core::slice::from_raw_parts(file.data, file.data_len) };
        let remaining = &data[file.offset..];
        let len = remaining.len().min(buf.len());
        buf[..len].copy_from_slice(&remaining[..len]);
        file.offset += len;
        Ok(len)
    }

    fn seek(&self, file: &mut File, pos: file::SeekFrom) -> Result<(), file::Error> {
        match pos {
            file::SeekFrom::Start(offset) => {
                if offset > file.data_len {
                    return Err(file::Error::UnexpectedEof);
                }
                file.offset = offset;
            }
            file::SeekFrom::Current(offset) => {
                let new_offset = file.offset
                    .checked_add_signed(offset)
                    .ok_or(file::Error::InvalidInput)?;
                if new_offset > file.data_len {
                    return Err(file::Error::UnexpectedEof);
                }
                file.offset = new_offset;
            }
        }
        Ok(())
    }
}

/// Static instance of IfsFileOps for use with File.
pub static IFS_FILE_OPS: IfsFileOps = IfsFileOps;

pub fn ifs_mount(initramfs: &'static [u8]) {
    let mut entries = Vec::new();
    let mut pos = 0;

    while pos + 110 <= initramfs.len() {
        let hdr = &initramfs[pos..];
        if &hdr[0..6] != b"070701" {
            break;
        }

        let namesize = parse_hex(&hdr[94..102]);
        let filesize = parse_hex(&hdr[54..62]);

        let name_start = pos + 110;
        let name_end = name_start + namesize;
        let filename = &initramfs[name_start..name_end - 1]; // strip null terminator
        let filename_str = core::str::from_utf8(filename).unwrap();

        if filename_str == "TRAILER!!!" {
            break;
        }

        let data_start = align_up(name_end, 4);
        let data_end = data_start + filesize;

        entries.push(FileEntry {
            path: format!("/{}", filename_str),
            data: &initramfs[data_start..data_end],
        });

        pos = align_up(data_end, 4);
    }

    unsafe {
        FILES.write(entries);
    }
}

fn align_up(x: usize, align: usize) -> usize {
    (x + align - 1) & !(align - 1)
}

fn parse_hex(bytes: &[u8]) -> usize {
    usize::from_str_radix(core::str::from_utf8(bytes).unwrap(), 16).unwrap()
}

pub fn ifs_open(path: &str) -> Result<File, &'static str> {
    let files = unsafe { &*FILES.as_ptr() };
    let entry = files.iter().find(|f| f.path == path).ok_or("File not found")?;
    Ok(File::with_data(&IFS_FILE_OPS, entry.data))
}


#[cfg(test)]
mod tests {
    extern crate alloc;
    use alloc::vec::Vec;
    use alloc::boxed::Box;
    use super::*;
    use crate::file::*;
    use crate::vfs;

    fn pad4(len: usize) -> usize {
        (len + 3) & !3
    }

    fn make_test_image(name: &str, data: &[u8]) -> &'static [u8] {
        //init_test_alloc();
        let mut buf = Vec::new();
        let name_cstr = format!("{}\0", name);
        let namesize = name_cstr.len();
        let filesize = data.len();

        // Write minimal CPIO "newc" header (110 bytes total)
        buf.extend_from_slice(b"070701"); // c_magic
        buf.extend_from_slice(&[b'0'; 110 - 6]); // fill rest of header with '0's
        buf[54..62].copy_from_slice(format!("{:08X}", filesize).as_bytes()); // c_filesize
        buf[94..102].copy_from_slice(format!("{:08X}", namesize).as_bytes()); // c_namesize

        buf.extend_from_slice(name_cstr.as_bytes());
        while buf.len() % 4 != 0 {
            buf.push(0);
        }

        buf.extend_from_slice(data);
        while buf.len() % 4 != 0 {
            buf.push(0);
        }

        // Append "TRAILER!!!" entry
        buf.extend_from_slice(b"070701");
        buf.extend_from_slice(&[b'0'; 110 - 6]);
        let buf_len = buf.len();
        buf[94 + buf_len - 110..102 + buf_len - 110]
            .copy_from_slice(b"0000000B");
        buf.extend_from_slice(b"TRAILER!!!\0");
        while buf.len() % 4 != 0 {
            buf.push(0);
        }

        Box::leak(buf.into_boxed_slice())
    }

    #[test_case]
    fn test_seek_and_read() {
        println!("Testing seek and read...");

        let data = b"hello world";
        let image = make_test_image("test.txt", data);
        ifs_mount(image);

        let mut file = ifs_open("/test.txt").unwrap();

        let mut buf = [0u8; 5];
        assert_eq!(vfs::vfs_read(&mut file, &mut buf).unwrap(), 5);
        assert_eq!(&buf, b"hello");

        vfs::vfs_seek(&mut file, SeekFrom::Start(6)).unwrap();
        assert_eq!(vfs::vfs_read(&mut file, &mut buf).unwrap(), 5);
        assert_eq!(&buf, b"world");
    }

    #[test_case]
    fn test_seek_beyond_end() {
        println!("Testing seek beyond end...");

        let data = b"short";
        let image = make_test_image("tiny.txt", data);
        ifs_mount(image);

        let mut file = ifs_open("/tiny.txt").unwrap();
        let result = vfs::vfs_seek(&mut file, SeekFrom::Start(1000));
        assert!(matches!(result, Err(Error::UnexpectedEof)));
    }

    #[test_case]
    fn test_seek_negative() {
        println!("Testing seek negative...");

        let data = b"12345678";
        let image = make_test_image("back.txt", data);
        ifs_mount(image);

        let mut file = ifs_open("/back.txt").unwrap();
        let result = vfs::vfs_seek(&mut file, SeekFrom::Current(-10));
        assert!(matches!(result, Err(Error::InvalidInput)));
    }
}
