use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::mem::MaybeUninit;
use core::str::Utf8Error;

static mut FILES: MaybeUninit<Vec<FileEntry>> = MaybeUninit::uninit();

struct FileEntry {
    path: String,
    data: &'static [u8],
}

pub struct IfsHandle {
    data: &'static [u8],
    offset: usize,
}

pub trait Read {
    type Error;

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error>;

    fn read_to_string(&mut self, out: &mut alloc::string::String) -> Result<(), Self::Error>
    where
        Self::Error: From<core::str::Utf8Error>,
    {
        let mut buf = [0u8; 256];
        loop {
            let len = self.read(&mut buf)?;
            if len == 0 {
                break;
            }
            out.push_str(core::str::from_utf8(&buf[..len])?);
        }
        Ok(())
    }
}

impl Read for IfsHandle {
    type Error = Utf8Error;

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let remaining = &self.data[self.offset..];
        let len = remaining.len().min(buf.len());
        buf[..len].copy_from_slice(&remaining[..len]);
        self.offset += len;
        Ok(len)
    }
}

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

pub fn ifs_open(path: &str) -> Result<IfsHandle, &'static str> {
    let files = unsafe { &*FILES.as_ptr() };
    let file = files.iter().find(|f| f.path == path).ok_or("File not found")?;

    Ok(IfsHandle { data: file.data, offset: 0 })
}
