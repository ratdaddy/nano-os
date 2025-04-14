//! A minimal RISC-V kernel written in Rust using `no_std`,
//! built to run in S-mode via U-Boot on a board like the Nano KVM Lite.

#![no_std]
#![no_main]
#![feature(naked_functions)]

mod trampoline;

#[macro_use]
mod console;

mod dtb;
mod page_allocator;
//mod page_mapper;

#[no_mangle]
fn rust_main(hart_id: usize, dtb_ptr: *const u8, kernel_phys_start: usize, kernel_phys_end: usize) -> ! {
     unsafe {
        core::arch::asm!(
            "csrw stvec, {}",
            in(reg) trap_handler as usize,
        );
     }

    console::sbi_console_putchar(b'*');
    console::sbi_console_putchar(b'*');
    console::sbi_console_putchar(b'*');
    console::sbi_console_putchar(b'\n');

    println!("Hello, world!");

    println!("Hart ID: {}", hart_id);

    println!("Kernel physical start: {:#x}", kernel_phys_start);
    println!("Kernel physical end: {:#x}", kernel_phys_end);

    #[cfg(feature = "dtb_raw")]
    {
        println!("dtb pointer: {:?}", dtb_ptr);
        let dtb_context;
        unsafe { dtb_context = dtb::parse_dtb(dtb_ptr); }
        println!("DTB size: {:#x}", dtb_context.total_size);
        hex_dump(dtb_ptr, 128);
    }

    #[cfg(feature="print_dtb")]
    unsafe {
        println!("DTB structure");
        dtb::print_dtb(dtb_ptr);
    }

    unsafe {
        dtb::check_memory_layout(dtb_ptr, kernel_phys_start);
    }

    let usable_memory;
    unsafe {
        usable_memory = dtb::get_usable_memory(dtb_ptr).expect("DTB doesn't have a memory node");
        println!("Usable memory: {:#x} - {:#x}", usable_memory.base, usable_memory.base + usable_memory.size);
    }

    unsafe {
        page_allocator::init(kernel_phys_end, usable_memory.base + usable_memory.size);
    }

    println!("Page allocator initialized: {} pages ({} free)",
            page_allocator::total_page_count(),
            page_allocator::free_page_count());

    /*
    let page_mapper = page_mapper::PageMapper::new();

    page_mapper.map_range(
        usable_memory.base,
        usable_memory.base,
        usable_memory.size,
        page_mapper::PageFlags::READ.union(page_mapper::PageFlags::WRITE).union(page_mapper::PageFlags::EXECUTE).union(page_mapper::PageFlags::ACCESSED).union(page_mapper::PageFlags::DIRTY),
        page_mapper::PageSize::Size2M,
    );

    println!("Memory mapped at root table: {:#x}", page_mapper.root_table as usize);

    let ppn = page_mapper.root_table as usize >> 12;
    let satp_value = (8 << 60) | ppn;

    println!("Switching to memory map with SATP value: {:#x}", satp_value);

    unsafe {
        core::arch::asm!(
            "csrw satp, {0}",
            "sfence.vma zero, zero",
            in(reg) satp_value,
            options(nostack)
        );
    }

    println!("Successfully switched to using memory map");
    */

    loop {
        unsafe { core::arch::asm!("wfi") }
    }
}

#[allow(dead_code)]
fn hex_dump(base: *const u8, length: usize) {
    unsafe {
        for offset in (0..length).step_by(16) {
            let line_addr = base.add(offset);
            print!("{:08p}:", line_addr);

            let mut ascii = [b'.'; 16];

            for i in 0..16 {
                let addr = base.add(offset + i);
                let byte = core::ptr::read_volatile(addr);
                print!(" {:02x}", byte);

                ascii[i] = if byte.is_ascii_graphic() || byte == b' ' {
                    byte
                } else {
                    b'.'
                };
            }

            let ascii_str = core::str::from_utf8_unchecked(&ascii);
            println!("  {}", ascii_str);
        }
    }
}

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    println!("Panic: {}", _info);
    loop {
        unsafe { core::arch::asm!("wfi") }
    }
}

#[no_mangle]
extern "C" fn trap_handler() {
    let scause: usize;
    let sepc: usize;
    let stval: usize;

    unsafe {
        core::arch::asm!(
            "csrr {0}, scause",
            "csrr {1}, sepc",
            "csrr {2}, stval",
            out(reg) scause,
            out(reg) sepc,
            out(reg) stval,
        );
    }

    println!("*** TRAP ***");
    println!("scause = {:#x}", scause);
    println!("sepc   = {:#x}", sepc);
    println!("stval  = {:#x}", stval);

    // Halt the system
    loop {
        unsafe { core::arch::asm!("wfi"); }
    }
}
