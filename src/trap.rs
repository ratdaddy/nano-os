use crate::amo;
//use crate::kernel_main;
use crate::kernel_memory_map;
use crate::syscall;

extern "C" {
    pub fn trap_entry();
}

core::arch::global_asm!(
    ".section .process_trampoline",
    ".global trap_entry",
    "trap_entry:",
    // Swap sp with trampoline trap frame pointer
    "csrrw sp, sscratch, sp",
    // Save t0
    "sd t0, TTF_T0(sp)",
    // Save sp
    "csrr t0, sscratch",
    "sd t0, TTF_USER_SP(sp)",
    // Put trampoline trap frame pointer back in sscratch
    "csrw sscratch, sp",
    // Set kernel mmap
    "ld t0, TTF_KERNEL_SATP(sp)",
    "csrw satp, t0",
    // T-Head C906: flush caches for SATP switch (user -> kernel)
    // See notes/thead-c906-memory-guide.md for cache instruction details
    "ld t0, TTF_IS_LICHEE_RVNANO(sp)",
    "beqz t0, 1f",
    ".long 0x0030000b",   // th.dcache.ciall - clean and invalidate D-cache
    ".long 0x0100000b",   // th.icache.iall  - invalidate I-cache
    "1: sfence.vma zero, zero",
    // Set kernel stack pointer but save trampoline trap frame in t0
    "mv t0, sp",
    "ld sp, TTF_KERNEL_SP(sp)",
    // save process sp and t0 on process trap frame
    "sd t1, PTF_T1(sp)",
    "ld t1, TTF_USER_SP(t0)",
    "sd t1, PTF_SP(sp)",
    "ld t1, TTF_T0(t0)",
    "sd t1, PTF_T0(sp)",
    // Save general purpose registers
    "sd ra, PTF_RA(sp)",
    "sd gp, PTF_GP(sp)",
    "sd tp, PTF_TP(sp)",
    "sd t2, PTF_T2(sp)",
    "sd s0, PTF_S0(sp)",
    "sd s1, PTF_S1(sp)",
    "sd a0, PTF_A0(sp)",
    "sd a1, PTF_A1(sp)",
    "sd a2, PTF_A2(sp)",
    "sd a3, PTF_A3(sp)",
    "sd a4, PTF_A4(sp)",
    "sd a5, PTF_A5(sp)",
    "sd a6, PTF_A6(sp)",
    "sd a7, PTF_A7(sp)",
    "sd s2, PTF_S2(sp)",
    "sd s3, PTF_S3(sp)",
    "sd s4, PTF_S4(sp)",
    "sd s5, PTF_S5(sp)",
    "sd s6, PTF_S6(sp)",
    "sd s7, PTF_S7(sp)",
    "sd s8, PTF_S8(sp)",
    "sd s9, PTF_S9(sp)",
    "sd s10, PTF_S10(sp)",
    "sd s11, PTF_S11(sp)",
    "sd t3, PTF_T3(sp)",
    "sd t4, PTF_T4(sp)",
    "sd t5, PTF_T5(sp)",
    "sd t6, PTF_T6(sp)",
    // Save sepc
    "csrr t0, sepc",
    "sd t0, PTF_SEPC(sp)",
    // Save scause
    "csrr t0, scause",
    "sd t0, PTF_SCAUSE(sp)",
    // save stval
    "csrr t0, stval",
    "sd t0, PTF_STVAL(sp)",
    // Save sstatus
    "csrr t0, sstatus",
    "sd t0, PTF_SSTATUS(sp)",
    // set process trap frame as argument to trap handler
    "mv a0, sp",

    // Handle trap
    "call trap_handler",

    // set sp to process trap frame
    "mv sp, a0",
    // Restore registers
    "ld ra, PTF_RA(sp)",
    "ld gp, PTF_GP(sp)",
    "ld tp, PTF_TP(sp)",
    "ld t1, PTF_T1(sp)",
    "ld t2, PTF_T2(sp)",
    "ld s0, PTF_S0(sp)",
    "ld s1, PTF_S1(sp)",
    "ld a0, PTF_A0(sp)",
    "ld a1, PTF_A1(sp)",
    "ld a2, PTF_A2(sp)",
    "ld a3, PTF_A3(sp)",
    "ld a4, PTF_A4(sp)",
    "ld a5, PTF_A5(sp)",
    "ld a6, PTF_A6(sp)",
    "ld a7, PTF_A7(sp)",
    "ld s2, PTF_S2(sp)",
    "ld s3, PTF_S3(sp)",
    "ld s4, PTF_S4(sp)",
    "ld s5, PTF_S5(sp)",
    "ld s6, PTF_S6(sp)",
    "ld s7, PTF_S7(sp)",
    "ld s8, PTF_S8(sp)",
    "ld s9, PTF_S9(sp)",
    "ld s10, PTF_S10(sp)",
    "ld s11, PTF_S11(sp)",
    "ld t3, PTF_T3(sp)",
    "ld t4, PTF_T4(sp)",
    "ld t5, PTF_T5(sp)",
    "ld t6, PTF_T6(sp)",
    // set trap return address and status
    "ld t0, PTF_PC(sp)",
    "csrw sepc, t0",
    "ld t0, PTF_SSTATUS(sp)",
    "csrw sstatus, t0",
    // change back to process mmap
    "ld t0, PTF_SATP(sp)",
    "csrw satp, t0",
    // load trampoline stack frame back into sp
    "csrr sp, sscratch",
    // T-Head C906: flush caches for SATP switch (kernel -> user)
    // See notes/thead-c906-memory-guide.md for cache instruction details
    "ld t0, TTF_IS_LICHEE_RVNANO(sp)",
    "beqz t0, 1f",
    ".long 0x0030000b",   // th.dcache.ciall - clean and invalidate D-cache
    ".long 0x0100000b",   // th.icache.iall  - invalidate I-cache
    "1: sfence.vma zero, zero",
    // Restore sp, t0
    "ld t0, TTF_T0(sp)",
    "ld sp, TTF_USER_SP(sp)",

    // return from trap
    "sret",
);

#[no_mangle]
#[link_section = ".process_trampoline"]
extern "C" fn trap_handler(tf: &mut types::ProcessTrapFrame) -> usize {
    let sepc = tf.sepc;
    let scause = tf.scause;
    let stval = tf.stval;

    println!(
        "Trap handler called: scause: {:#x}, stval: {:#x}, sepc: {:#x}",
        scause, stval, sepc
    );
    const AMO_FAULT: usize = 7;
    const USER_ECALL: usize = 8;
    const LOAD_PAGE_FAULT: usize = 13;
    const STORE_PAGE_FAULT: usize = 15;
    const SUPERVISOR_EXTERNAL_INTERRUPT: usize = 0x8000000000000009;

    //match scause & 0xff {
    match scause {
        AMO_FAULT => {
            amo::handle_amo_fault(tf);
        }
        LOAD_PAGE_FAULT | STORE_PAGE_FAULT => {
            if !kernel_memory_map::grow_stack_on_page_fault(stval) {
                panic!("Unhandled page fault at address {:#x} (scause: {})", stval, scause);
            }
        }
        USER_ECALL => syscall::handle(tf),
        SUPERVISOR_EXTERNAL_INTERRUPT => {
            panic!("Interrupt occurred");
        }
        _ => {
            panic!("Unhandled trap: scause: {:#x}, stval: {:#x}", scause, stval);
        }
    }

    tf as *const _ as usize
}
