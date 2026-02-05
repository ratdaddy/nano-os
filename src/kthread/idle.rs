use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec;

use crate::riscv;
use crate::thread;

use types::ThreadContext;

const IDLE_STACK_SIZE: usize = 4096;

pub fn init() {
    let stack = vec![0u8; IDLE_STACK_SIZE];
    let sp = stack.as_ptr() as usize + IDLE_STACK_SIZE;

    let thread = Box::new(thread::Thread {
        id: 0,
        state: thread::ThreadState::Ready,
        context: ThreadContext {
            sp,
            ra: idle_entry as usize,
            ..ThreadContext::default()
        },
        stack,
        inbox: VecDeque::new(),
        process: None,
    });

    let ptr = Box::into_raw(thread);
    thread::set_idle_thread(ptr, idle_entry as usize);
}

fn idle_entry() -> ! {
    // Enable all S-mode interrupt sources in sie register
    unsafe {
        core::arch::asm!("csrw sie, {}", in(reg) riscv::SIE_ALL);
    }

    loop {
        unsafe {
            // All in one asm block to ensure no instructions between them
            core::arch::asm!(
                "csrs sstatus, {sie}",      // Enable interrupts
                "wfi",                       // Wait for interrupt
                "csrc sstatus, {sie}",      // Disable interrupts
                sie = in(reg) riscv::SSTATUS_SIE,
            );
        }
        thread::schedule_if_ready();
    }
}
