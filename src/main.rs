//! A minimal RISC-V kernel written in Rust using `no_std`,
//! built to run in S-mode via U-Boot on a board like the Nano KVM Lite.

#![no_std]
#![no_main]
#![feature(naked_functions)]

mod trampoline;

#[macro_use]
mod console;

mod memory;
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

    let dtb_context = unsafe { dtb::parse_dtb(dtb_ptr) };
    println!("DTB pointer: {:?}", dtb_ptr);
    println!("DTB size: {:#x}", dtb_context.total_size);

    #[cfg(feature = "dtb_raw")]
    {
        hex_dump(dtb_ptr, 128);
    }

    #[cfg(feature="print_dtb")]
    unsafe {
        println!("DTB structure");
        dtb::print_dtb(dtb_ptr);
    }

    /*
    unsafe {
        dtb::check_memory_layout(dtb_ptr, kernel_phys_start);
    }

    let usable_memory;
    unsafe {
        usable_memory = dtb::get_usable_memory(dtb_ptr).expect("DTB doesn't have a memory node");
        println!("Usable memory: {:#x} - {:#x}", usable_memory.base, usable_memory.base + usable_memory.size);
    }
    */

    const MAX_RESERVED_MEMORY: usize = 16;
    const MAX_USABLE_MEMORY: usize = MAX_RESERVED_MEMORY + 1;

    let mut reserved_memory: heapless::Vec<memory::Region, MAX_RESERVED_MEMORY> = heapless::Vec::new();
    let mut usable_memory: heapless::Vec<memory::Region, MAX_USABLE_MEMORY> = heapless::Vec::new();

    let memory = unsafe {
        dtb::collect_memory_map(dtb_ptr, &mut reserved_memory).expect("Failed to collect memory map")
    };

    println!("Memory {:#x} - {:#x}", memory.start, memory.end);

    let _ = reserved_memory.push(memory::Region {
        start: memory.start,
        end: memory::align_up(kernel_phys_end),
    });

    let _ = reserved_memory.push(memory::Region {
        start: memory::align_down(dtb_ptr as usize),
        end: memory::align_up(dtb_ptr as usize + dtb_context.total_size),
    });

    println!("Reserved memory regions:");
    for region in reserved_memory.iter() {
        println!("  {:#x} - {:#x}", region.start, region.end);
    }

    memory::compute_usable_regions(memory, &mut reserved_memory, &mut usable_memory);

    println!("Usable memory regions:");
    for region in usable_memory.iter() {
        println!("  {:#x} - {:#x}", region.start, region.end);
    }

    unsafe {
        page_allocator::init(&usable_memory);
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
