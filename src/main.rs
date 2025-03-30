//! A minimal RISC-V kernel written in Rust using `no_std`,
//! built to run in S-mode via U-Boot on a board like the Nano KVM Lite.
//! Uses `sbi_console_putchar` to print "Hello, world!".

#![no_std]
#![no_main]

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
    sbi_putchar(b'*');
    sbi_putchar(b'*');
    sbi_putchar(b'*');
    sbi_putchar(b'\n');

    match sbi_debug_console_write(b"hello, world\n") {
        Ok(_) => {
            sbi_debug_console_write(b"no errors, no worries\n").ok();
        },
        Err(_) => {
            sbi_debug_console_write(b"error writing to console\n").ok();
        },
    }

    loop {
        unsafe { core::arch::asm!("wfi") }
    }
}

const DBCN_EID: u64 = 0x4442434E;

pub fn sbi_debug_console_write(data: &[u8]) -> Result<usize, i64> {
    let len = data.len();
    let ptr = data.as_ptr();
    let status: i64;
    let written_len: usize;

    unsafe {
        core::arch::asm!(
            "ecall",
            in("a7") DBCN_EID,
            in("a6") 0,
            in("a0") len,
            in("a1") ptr,
            in("a2") 0,
            lateout("a0") status,
            lateout("a1") written_len,
            options(nostack, nomem),
        );
    }

    if status < 0 {
        Err(status)
    } else {
        Ok(written_len)
    }
}

#[inline(always)]
fn sbi_putchar(ch: u8) {
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") ch as usize,
            in("a7") 0x01,
            in("a6") 0x00,
            lateout("a0") _,
            options(nostack, nomem),
        );
    }
}

/*
#[no_mangle]
extern "C" fn trap_entry() {
    unsafe {
        core::arch::asm!(
            "csrr t0, sepc",
            "addi t0, t0, 4",
            "csrw sepc, t0",
            "sret",
            options(noreturn)
        );
    }
}
*/

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    sbi_debug_console_write(b"Panic!\n").ok();
    loop {
        unsafe { core::arch::asm!("wfi") }
    }
}
