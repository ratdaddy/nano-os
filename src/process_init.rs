use core::sync::atomic::Ordering;

use crate::dtb;
use crate::initramfs;
use crate::process;
use crate::process_memory_map;
use crate::process_trampoline;
use crate::read_elf;

/// Mount initramfs, load the init process ELF, and enter user mode.
/// Never returns.
pub fn run_init_process() -> ! {
    let initrd_start = dtb::INITRD_START.load(Ordering::Relaxed);
    let initrd_len = dtb::INITRD_END.load(Ordering::Relaxed) - initrd_start;

    let slice = unsafe { core::slice::from_raw_parts(initrd_start as *const _, initrd_len) };
    initramfs::ifs_mount(slice);

    let mut handle = initramfs::ifs_open("/prog_example").unwrap();
    let header = read_elf::read_elf64_header(&mut handle).unwrap();
    println!("Reading ELF for /prog_example");
    println!("Entry point:     {:#x}", header.e_entry);
    println!("PH offset:       {:#x}", header.e_phoff);
    println!("PH entry size:   {}", header.e_phentsize);
    println!("PH count:        {}", header.e_phnum);

    let program_headers = read_elf::read_program_headers(&mut handle, &header).unwrap();

    for ph in &program_headers {
        println!("Program header: type: {:#x} offset: {:#x} virt addr:{:#x}-{:#x} file size: {:#x} mem size: {:#x}",
            ph.p_type, ph.p_offset, ph.p_vaddr, ph.p_vaddr + ph.p_memsz, ph.p_filesz, ph.p_memsz);
    }

    println!();

    let mut handle = initramfs::ifs_open("/prog_example").unwrap();

    println!();

    process::init();

    let context = process::create();

    process_memory_map::init_from_elf(&mut handle, context);

    println!("Process context initialized");

    unsafe {
        println!("Entering process trampoline");
        process_trampoline::enter_process(context);
    }
}
