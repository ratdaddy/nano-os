use core::sync::atomic::Ordering;

use crate::dtb;
use crate::initramfs;
use crate::io::Read;

pub fn inspect_initramfs() {
    let initrd_start = dtb::INITRD_START.load(Ordering::Relaxed);
    let initrd_end = dtb::INITRD_END.load(Ordering::Relaxed);
    let initrd_len = initrd_end - initrd_start;
    let start = initrd_start as *const u8;

    unsafe {
        // Read the first 6 bytes as the magic number
        let magic = core::str::from_utf8_unchecked(core::slice::from_raw_parts(start, 6));
        if magic != "070701" {
            println!("Invalid cpio magic: {}", magic);
            return;
        }
        println!("CPIO magic: {}", magic);

        // Grab more interesting fields
        let namesize_str =
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(start.add(94), 8));
        let mode_str =
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(start.add(14), 8));
        let filesize_str =
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(start.add(102), 8));

        let namesize = usize::from_str_radix(namesize_str, 16).unwrap_or(0);
        let mode = u32::from_str_radix(mode_str, 16).unwrap_or(0);
        let filesize = usize::from_str_radix(filesize_str, 16).unwrap_or(0);

        println!("Mode: {:o}", mode);
        println!("File size: {} bytes", filesize);
        println!("Name size: {} bytes", namesize);

        // Optionally print the filename too
        let name_start = start.add(110);
        let name_bytes = core::slice::from_raw_parts(name_start, namesize);
        if let Ok(name) = core::str::from_utf8(name_bytes) {
            println!("Filename: {}", name.trim_end_matches('\0'));
        }
    }

    // Mount and read /etc/motd
    let slice = unsafe { core::slice::from_raw_parts(start, initrd_len) };
    initramfs::ifs_mount(slice);

    match initramfs::ifs_open("/etc/motd") {
        Ok(mut handle) => {
            let mut contents = alloc::string::String::new();
            let _ = handle.read_to_string(&mut contents);
            println!("Contents of /etc/motd: {}", contents);
        }
        Err(e) => println!("/etc/motd: {}", e),
    }
}
