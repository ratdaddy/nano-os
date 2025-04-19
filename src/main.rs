//! A minimal RISC-V kernel written in Rust using `no_std`,
//! built to run in S-mode via U-Boot on a board like the Nano KVM Lite.

#![no_std]
#![no_main]
#![feature(naked_functions)]

mod trampoline;

#[macro_use]
mod console;

mod dtb;
mod kernel_main;
mod kernel_memory_map;
mod memory;
mod page_allocator;
mod page_mapper;

#[no_mangle]
fn rust_main(
    hart_id: usize,
    dtb_ptr: *const u8,
    kernel_phys_start: usize,
    kernel_phys_end: usize,
) -> ! {
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

    println!("Welcome to the Nano KVM Lite kernel!");

    println!("Hart ID: {}", hart_id);

    println!("Kernel physical start: {:#x}", kernel_phys_start);
    println!("Kernel physical end: {:#x}", kernel_phys_end);

    zero_bss();

    let dtb_context = unsafe { dtb::parse_dtb(dtb_ptr) };
    println!("DTB pointer: {:?}", dtb_ptr);
    println!("DTB size: {:#x}", dtb_context.total_size);

    #[cfg(feature = "print_dtb")]
    unsafe {
        println!("DTB structure");
        dtb::print_dtb(dtb_ptr);
    }

    let memory = page_allocator::init(dtb_ptr, kernel_phys_end);

    let root_table = kernel_memory_map::init(memory);

    let ppn = root_table as usize >> 12;
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

    unsafe {
        core::arch::asm!("mv sp, {}", in(reg) kernel_memory_map::KERNEL_STACK_START);
    }

    kernel_main::kernel_main();

    panic!("Kernel main returned unexpectedly");
}

fn zero_bss() {
    extern "C" {
        static mut _bss_start: u8;
        static mut _bss_end: u8;
    }

    unsafe {
        let bss_start = core::ptr::addr_of!(_bss_start) as usize;
        let bss_end = core::ptr::addr_of!(_bss_end) as usize;
        let size = bss_end - bss_start;

        println!("Zeroing BSS segment: {:#x} - {:#x} (size: {})", bss_start, bss_end, size);

        core::ptr::write_bytes(bss_start as *mut u8, 0, size);
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
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}
