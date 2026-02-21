//! A minimal RISC-V kernel written in Rust using `no_std`,
//! built to run in S-mode via U-Boot on a board like the Nano KVM Lite.

#![no_std]
#![no_main]
#![feature(naked_functions)]
#![feature(alloc_error_handler)]
#![feature(custom_test_frameworks)]
#![test_runner(test::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

mod trampoline;

#[macro_use]
#[cfg_attr(test, allow(dead_code))]
mod console;

#[cfg(not(test))]
#[macro_use]
mod kprint;

#[cfg(not(test))]
mod kthread;

#[cfg(not(test))]
mod amo;
#[cfg(not(test))]
mod bytes;
#[cfg(not(test))]
mod block;
#[cfg(not(test))]
mod collections;
#[cfg(not(test))]
mod asm_offsets;
#[cfg(not(test))]
mod cpu_info;
#[cfg(not(test))]
mod demos;
#[cfg_attr(test, allow(dead_code))]
mod dtb;
#[cfg_attr(test, allow(dead_code))]
mod file;
mod chardev;
mod fs;
mod vfs;
#[cfg_attr(test, allow(dead_code))]
mod kernel_allocator;
#[cfg(not(test))]
mod kernel_main;
#[cfg(not(test))]
mod kernel_memory_map;
#[cfg(not(test))]
mod kernel_trap;
#[cfg_attr(test, allow(dead_code))]
mod memory;
#[cfg(not(test))]
mod page_allocator;
#[cfg(not(test))]
mod page_mapper;
#[cfg(not(test))]
mod drivers;
#[cfg(not(test))]
mod process;
#[cfg(not(test))]
mod process_memory_map;
#[cfg(not(test))]
mod process_trampoline;
#[cfg(not(test))]
mod read_elf;
#[cfg(not(test))]
mod riscv;
mod test;
#[cfg(not(test))]
mod thread;
#[cfg(not(test))]
mod trap;
#[cfg(not(test))]
mod syscall;

use core::panic::PanicInfo;

#[cfg(not(test))]
#[no_mangle]
fn rust_main(
    hart_id: usize,
    dtb_ptr: *const u8,
    image_phys_start: usize,
    image_phys_end: usize,
) -> ! {
    unsafe {
        core::arch::asm!(
            "csrw stvec, {}",
            in(reg) boot_trap_handler as usize,
        );
    }

    console::sbi_console_putchar(b'*');
    console::sbi_console_putchar(b'*');
    console::sbi_console_putchar(b'*');
    console::sbi_console_putchar(b'\n');

    println!("Welcome to the Nano KVM Lite kernel!");

    println!("Hart ID: {}", hart_id);

    println!("Image physical start: {:#x}", image_phys_start);
    println!("Image physical end: {:#x}", image_phys_end);

    zero_bss();

    let dtb_context = unsafe { dtb::parse_dtb(dtb_ptr) };
    println!("DTB pointer: {:?}", dtb_ptr);
    println!("DTB size: {:#x}", dtb_context.total_size);

    #[allow(dead_code)]
    #[cfg(feature = "print_dtb")]
    unsafe {
        println!("DTB structure");
        dtb::print_dtb(dtb_ptr);
    }

    dtb::detect_cpu_type(dtb_ptr);
    dtb::parse_timebase_frequency(dtb_ptr);

    cpu_info::show_cpu_info();

    let memory = page_allocator::init(dtb_ptr, image_phys_end);

    kernel_memory_map::init(memory);

    unsafe {
        core::arch::asm!("mv sp, {}", in(reg) kernel_memory_map::KERNEL_STACK_START);
    }

    kernel_main::kernel_main();
}

#[cfg(not(test))]
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

#[alloc_error_handler]
fn alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout);
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    println!("\x1b[31mPanic: {}\x1b[0m", _info);
    loop {
        unsafe { core::arch::asm!("wfi") }
    }
}

#[no_mangle]
extern "C" fn boot_trap_handler() {
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

    println!("\x1b[31m*** TRAP ***");
    println!("scause = {:#x}", scause);
    println!("sepc   = {:#x}", sepc);
    println!("stval  = {:#x}\x1b[0m", stval);

    // Halt the system
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}

#[cfg(test)]
#[no_mangle]
fn rust_main() -> ! {
    unsafe {
        core::arch::asm!(
            "csrw stvec, {}",
            in(reg) boot_trap_handler as usize,
        );
    }
    test_main();
    println!("\x1b[32mAll tests passed!\x1b[0m");
    test::exit_qemu();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("\x1b[31mTest panic: {info}\x1b[0m");
    test::exit_qemu();
}
