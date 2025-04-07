//! A minimal RISC-V kernel written in Rust using `no_std`,
//! built to run in S-mode via U-Boot on a board like the Nano KVM Lite.

#![no_std]
#![no_main]

#[macro_use]
mod console;

mod dtb;
mod page_allocator;

use core::panic::PanicInfo;

extern "C" { static _stack_end: u8; }

#[no_mangle]
pub extern "C" fn _start(hart_id: usize, dtb_ptr: *const u8) -> ! {

    unsafe {
        core::arch::asm!(
            "la sp, {stack}",
            stack = sym _stack_end,
            options(nostack)
        );
    }

    rust_main(hart_id, dtb_ptr);
}

fn rust_main(hart_id: usize, dtb_ptr: *const u8) -> ! {
    console::sbi_console_putchar(b'*');
    console::sbi_console_putchar(b'*');
    console::sbi_console_putchar(b'*');
    console::sbi_console_putchar(b'\n');

    println!("Hello, world!");

    println!("Hart ID: {}", hart_id);

    #[cfg(feature = "dtb_raw")]
    {
        println!("A1 pointer: {:?}", dtb_ptr);
        hex_dump(dtb_ptr, 512);
    }

    #[cfg(feature="print_dtb")]
    unsafe {
        println!("DTB structure");
        dtb::print_dtb(dtb_ptr);
    }

    unsafe {
        dtb::check_memory_layout(dtb_ptr);
    }

    let usable_memory;
    unsafe {
        usable_memory = dtb::get_usable_memory(dtb_ptr).expect("DTB doesn't have a memory node");
        println!("Usable memory: {:#x} - {:#x}", usable_memory.base, usable_memory.base + usable_memory.size);
    }

    unsafe {
        page_allocator::init(&_stack_end as *const u8 as usize, usable_memory.base + usable_memory.size);
    }

    println!("Page allocator initialized: {} pages ({} free)",
            page_allocator::total_page_count(),
            page_allocator::free_page_count());

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

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    println!("Panic: {}", _info);
    loop {
        unsafe { core::arch::asm!("wfi") }
    }
}
