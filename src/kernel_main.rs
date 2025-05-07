use alloc::vec::Vec;
use alloc::vec;
use alloc::boxed::Box;
use core::sync::atomic::Ordering;

use crate::kernel_allocator;
use crate::dtb;
use crate::initramfs::{self, Read};

extern "C" {
    fn trap_entry();
}

pub fn kernel_main() {
    println!("In kernel_main");

    // Could reclaim pages used in original page map and early boot stack here

    unsafe {
        core::arch::asm!(
            "csrw stvec, {}",
            in(reg) trap_entry as usize,
        );
    }

    /*
    test_stack_allocation();

    test_alloc1();
    test_alloc2();
    unsafe { kernel_allocator::ALLOCATOR.dump_heap(); }
    */

    let initrd_start = dtb::INITRD_START.load(Ordering::Relaxed);
    let initrd_len = dtb::INITRD_END.load(Ordering::Relaxed) - initrd_start;
    inspect_initramfs(initrd_start as *const u8);

    let slice = unsafe { core::slice::from_raw_parts(initrd_start as *const _, initrd_len) };
    initramfs::ifs_mount(slice);

    let mut handle = initramfs::ifs_open("/etc/motd").unwrap();
    let mut contents = alloc::string::String::new();
    let _result = handle.read_to_string(&mut contents);

    println!("Contents of /etc/motd: {}", contents);

    loop {
        unsafe { core::arch::asm!("wfi") }
    }
}

pub fn inspect_initramfs(start: *const u8) {
    unsafe {
        // Read the first 6 bytes as the magic number
        let magic = core::str::from_utf8_unchecked(core::slice::from_raw_parts(start, 6));
        if magic != "070701" {
            println!("Invalid cpio magic: {}", magic);
            return;
        }
        println!("CPIO magic: {}", magic);

        // Grab more interesting fields
        let namesize_str = core::str::from_utf8_unchecked(core::slice::from_raw_parts(start.add(94), 8));
        let mode_str = core::str::from_utf8_unchecked(core::slice::from_raw_parts(start.add(14), 8));
        let filesize_str = core::str::from_utf8_unchecked(core::slice::from_raw_parts(start.add(102), 8));

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
}

fn test_stack_allocation() {
    let data = [42u8; 10 * 1024];

    // Touch the memory so it’s not optimized out
    let mut sum = 0u32;
    for &byte in &data {
        sum += byte as u32;
    }

    // Pass by value to copy onto the callee's stack
    consume_array(data);

    // Use result so the compiler doesn't optimize everything away
    println!("Sum: {}", sum);
}

fn consume_array(arr: [u8; 10 * 1024]) {
    let avg = arr.iter().map(|&b| b as u32).sum::<u32>() / arr.len() as u32;
    println!("Average: {}", avg);
}

#[allow(dead_code)]
#[repr(align(128))]
struct Align128(u8);

fn test_alloc1() {
    println!("\n*** Testing allocation ***");
    unsafe { kernel_allocator::ALLOCATOR.dump_heap(); }
    /*
    let _buffer1: Box<[u8]> = vec![0u8; 128].into_boxed_slice();
    let mut v = Vec::new();
    v.push(42);
    v.push(100);
    v.push(200);
    v.push(300);
    v.push(300);
    */
    let _buffer2: Box<[u8]> = vec![0u8; 109].into_boxed_slice();
    unsafe { kernel_allocator::ALLOCATOR.dump_heap(); }
    let _buffer3 = Box::new(Align128(255));
    //let _buffer2: Box<[u8]> = vec![0u8; 101].into_boxed_slice();
    unsafe { kernel_allocator::ALLOCATOR.dump_heap(); }
}

fn test_alloc2() {
    let mut v = Vec::new();
    v.push(42);
    unsafe { kernel_allocator::ALLOCATOR.dump_heap(); }
    let _buffer1: Box<[u8]> = vec![0u8; 16000].into_boxed_slice();
    let _buffer2: Box<[u8]> = vec![0u8; 4016].into_boxed_slice();
    let _buffer3: Box<[u8]> = vec![0u8; 128].into_boxed_slice();
    unsafe { kernel_allocator::ALLOCATOR.dump_heap(); }
}
