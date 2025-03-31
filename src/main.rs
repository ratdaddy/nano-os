//! A minimal RISC-V kernel written in Rust using `no_std`,
//! built to run in S-mode via U-Boot on a board like the Nano KVM Lite.

#![no_std]
#![no_main]

mod console;

use core::panic::PanicInfo;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    extern "C" {
        static _stack_end: u8;
    }

    unsafe {
        core::arch::asm!(
            "la sp, {stack}",
            stack = sym _stack_end,
            options(nostack)
        );
    }

    rust_main();
}

fn rust_main() -> ! {
    console::sbi_console_putchar(b'*');
    console::sbi_console_putchar(b'*');
    console::sbi_console_putchar(b'*');
    console::sbi_console_putchar(b'\n');

    println!("Hello, world!");

    loop {
        unsafe { core::arch::asm!("wfi") }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    println!("Panic: {}", _info);
    loop {
        unsafe { core::arch::asm!("wfi") }
    }
}
