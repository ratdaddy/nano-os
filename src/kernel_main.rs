use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use crate::dtb;
use crate::initramfs;
use crate::io::Read;
use crate::kernel_allocator;
use crate::page_mapper;
use crate::process_main;
use crate::process_memory_map;
use crate::process_trampoline;
use crate::read_elf;

extern "C" {
    pub fn trap_entry();
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

    let mut handle = initramfs::ifs_open("/prog_example").unwrap();
    let header = read_elf::read_elf64_header(&mut handle).unwrap();
    println!("Reading ELF for /prog_example");
    println!("Entry point:     {:#x}", header.e_entry);
    println!("PH offset:       {:#x}", header.e_phoff);
    println!("PH entry size:   {}", header.e_phentsize);
    println!("PH count:        {}", header.e_phnum);

    let program_headers = read_elf::read_program_headers(&mut handle, &header).unwrap();

    for ph in &program_headers {
        println!("Program header: type: {:#x} offset: {:#x} virt addr:{:#x} file size: {:#x} mem size: {:#x}",
            ph.p_type, ph.p_offset, ph.p_vaddr, ph.p_filesz, ph.p_memsz);
    }

    println!();

    let fn_ptr = process_main::process_main;
    println!("Process main function pointer: {:#x}", fn_ptr as usize);
    let context = &mut process_trampoline::ProcessContext {
        user_sp: process_memory_map::PROCESS_STACK_START,
        user_pc: fn_ptr as usize,
        user_status: 1 << 5,
        page_map: page_mapper::PageMapper::new(),
    };

    process_memory_map::init(&mut context.page_map);

    unsafe {
        println!("entering process trampoline");
        process_trampoline::enter_process(context);
    }

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
