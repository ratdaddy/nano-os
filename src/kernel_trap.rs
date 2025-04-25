use crate::kernel_memory_map;

core::arch::global_asm!(
    ".section .text",
    ".global trap_entry",
    "trap_entry:",
    // Save original sp
    "csrrw t1, sscratch, sp",
    // Load trap stack pointer
    "la t0, KERNEL_TRAP_STACK_START",
    "ld sp, 0(t0)",
    // Save a0, a1 to trap stack
    "addi sp, sp, -16",
    "sd a0, 0(sp)",
    "sd a1, 8(sp)",
    // Load trap args
    "csrr a0, scause",
    "csrr a1, stval",
    "call kernel_trap_handler",
    // Restore a0, a1
    "ld a0, 0(sp)",
    "ld a1, 8(sp)",
    "addi sp, sp, 16",
    // Restore original sp
    "csrrw sp, sscratch, t1",
    "sret",
);

#[no_mangle]
extern "C" fn kernel_trap_handler(scause: usize, stval: usize) {
    let sepc: usize;
    unsafe {
        core::arch::asm!("csrr {0}, sepc", out(reg) sepc);
    }

    println!("Trap handler called: scause: {:#x}, stval: {:#x}, sepc: {:#x}", scause, stval, sepc);
    const LOAD_PAGE_FAULT: usize = 13;
    const STORE_PAGE_FAULT: usize = 15;

    match scause & 0xff {
        LOAD_PAGE_FAULT | STORE_PAGE_FAULT => {
            if !kernel_memory_map::grow_stack_on_page_fault(stval) {
                panic!("Unhandled page fault at address {:#x} (scause: {})", stval, scause);
            }
        }
        _ => {
            panic!("Unhandled trap: scause: {:#x}, stval: {:#x}", scause, stval);
        }
    }
}
