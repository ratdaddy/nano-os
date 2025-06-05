//! Syscall handling logic.
//!
//! Syscalls will eventually be numerous so they are organized by broad
//! operation type into submodules.  This allows the kernel to grow
//! groups of related syscalls without creating a single massive file.
//!
//! The current groupings are:
//!  - `file`   : operations that act on file descriptors.
//!  - `memory` : memory management related syscalls.
//!  - `process`: process/thread management syscalls.
//!  - `signal` : signal related syscalls.

mod file;
mod memory;
mod process;
mod signal;

use crate::trap::TrapFrame;

/// Handle a syscall coming from user mode.
///
/// The trap frame contains the saved registers from the trapped
/// process.  The program counter provided is the address of the
/// trapping `ecall` instruction.  All syscall handlers must update the
/// return value in `a0` and advance the program counter past the
/// `ecall` instruction.
pub fn handle(tf: &mut TrapFrame) {
    let syscall_number = tf.registers.a7;
    println!("User ecall: syscall number: {}", syscall_number);
    match syscall_number {
        // ppoll, rt_sigaction, sigaltstack, rt_sigprocmask
        73 | 134 | 132 | 135 => signal::generic_stub(tf),
        96 => process::set_tid_address(tf),
        222 => memory::mmap(tf),
        214 => memory::brk(tf),
        64 => file::write(tf),
        _ => {
            println!("Unhandled syscall");
            loop {
                unsafe { core::arch::asm!("wfi") }
            }
        }
    }

    // Advance past the trapping instruction.
    tf.registers.pc = tf.sepc + 4;
}
