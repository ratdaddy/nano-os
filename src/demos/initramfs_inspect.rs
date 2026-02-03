use core::sync::atomic::Ordering;

use crate::dtb;
use crate::initramfs;
use crate::file_ops::FileOps;

pub fn inspect_initramfs() {
    let initrd_start = dtb::INITRD_START.load(Ordering::Relaxed);
    let initrd_end = dtb::INITRD_END.load(Ordering::Relaxed);
    let start = initrd_start as *const u8;

    println!("Initramfs location: {:#x} - {:#x}", initrd_start, initrd_end);

    unsafe {
        // Read the first 6 bytes as the magic number
        let magic = core::str::from_utf8_unchecked(core::slice::from_raw_parts(start, 6));
        if magic != "070701" {
            println!("Invalid cpio magic: {}", magic);
            return;
        }
        println!("CPIO magic: {}", magic);

        // Grab more interesting fields from first entry
        let namesize_str =
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(start.add(94), 8));
        let mode_str =
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(start.add(14), 8));
        let filesize_str =
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(start.add(54), 8));

        let namesize = usize::from_str_radix(namesize_str, 16).unwrap_or(0);
        let mode = u32::from_str_radix(mode_str, 16).unwrap_or(0);
        let filesize = usize::from_str_radix(filesize_str, 16).unwrap_or(0);

        println!("First entry - Mode: {:o}, File size: {}, Name size: {}", mode, filesize, namesize);

        let name_start = start.add(110);
        let name_bytes = core::slice::from_raw_parts(name_start, namesize);
        if let Ok(name) = core::str::from_utf8(name_bytes) {
            println!("First filename: {}", name.trim_end_matches('\0'));
        }
    }

    // Read /etc/motd (initramfs already mounted by kernel_main)
    match initramfs::ifs_open("/etc/motd") {
        Ok(mut handle) => {
            let mut contents = alloc::string::String::new();
            let _ = handle.read_to_string(&mut contents);
            println!("Contents of /etc/motd: {}", contents);
        }
        Err(e) => println!("/etc/motd: {}", e),
    }
}
