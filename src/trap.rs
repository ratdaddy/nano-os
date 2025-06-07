use crate::kernel_memory_map;
use cpu_types::{Registers, TrapFrame};
use crate::syscall;

core::arch::global_asm!(include_str!(concat!(env!("OUT_DIR"), "/trap_offsets.S")));

include!(concat!(env!("OUT_DIR"), "/trap_offsets.rs"));

core::arch::global_asm!(
    ".section .process_trampoline",
    ".global trap_entry",
    "trap_entry:",
    "csrrw sp, sscratch, sp",
    //Save caller-saved registers
    "sd ra,  TF_RA(sp)",
    "sd gp,  TF_GP(sp)",
    "sd tp,  TF_TP(sp)",
    "sd t0,  TF_T0(sp)",
    "csrr t0, sscratch",
    "sd t0,  TF_SP(sp)",
    "sd t1,  TF_T1(sp)",
    "sd t2,  TF_T2(sp)",
    "sd s0,  TF_S0(sp)",
    "sd s1,  TF_S1(sp)",
    "sd a0,  TF_A0(sp)",
    "sd a1,  TF_A1(sp)",
    "sd a2,  TF_A2(sp)",
    "sd a3,  TF_A3(sp)",
    "sd a4,  TF_A4(sp)",
    "sd a5,  TF_A5(sp)",
    "sd a6,  TF_A6(sp)",
    "sd a7,  TF_A7(sp)",
    "sd s2,  TF_S2(sp)",
    "sd s3,  TF_S3(sp)",
    "sd s4,  TF_S4(sp)",
    "sd s5,  TF_S5(sp)",
    "sd s6,  TF_S6(sp)",
    "sd s7,  TF_S7(sp)",
    "sd s8,  TF_S8(sp)",
    "sd s9,  TF_S9(sp)",
    "sd s10, TF_S10(sp)",
    "sd s11, TF_S11(sp)",
    "sd t3,  TF_T3(sp)",
    "sd t4,  TF_T4(sp)",
    "sd t5,  TF_T5(sp)",
    "sd t6,  TF_T6(sp)",
    "csrr t0, sepc",
    "sd t0,  TF_PC(sp)",
    "sd t0,  TF_SEPC(sp)",
    "csrr t0, sstatus",
    "sd t0,  TF_SSTATUS(sp)",
    "csrr t0, stval",
    "sd t0,  TF_STVAL(sp)",
    "csrr t0, scause",
    "sd t0,  TF_SCAUSE(sp)",
    "ld t0,  TF_KERNEL_SATP(sp)",
    "csrw satp, t0",
    "ld t0,  TF_IS_LICHEE_RVNANO(sp)",
    "beqz t0, 1f",
    ".long 0x0020000b",
    ".long 0x0190000b",
    "1: sfence.vma zero, zero",
    "ld sp, KERNEL_STACK_START",

    // Handle trap
    "call trap_handler",

    // Restore registers
    "mv t0, a0",

    "ld ra,TF_RA(t0)",
    "ld sp,TF_SP(t0)",
    "ld gp,TF_GP(t0)",
    "ld tp,TF_TP(t0)",
    // skip t0 & t1 for now
    "ld t2,TF_T2(t0)",
    "ld s0,TF_S0(t0)",
    "ld s1,TF_S1(t0)",
    "ld a0,TF_A0(t0)",
    "ld a1,TF_A1(t0)",
    "ld a2,TF_A2(t0)",
    "ld a3,TF_A3(t0)",
    "ld a4,TF_A4(t0)",
    "ld a5,TF_A5(t0)",
    "ld a6,TF_A6(t0)",
    "ld a7,TF_A7(t0)",
    "ld s2,TF_S2(t0)",
    "ld s3,TF_S3(t0)",
    "ld s4,TF_S4(t0)",
    "ld s5,TF_S5(t0)",
    "ld s6,TF_S6(t0)",
    "ld s7,TF_S7(t0)",
    "ld s8,TF_S8(t0)",
    "ld s9,TF_S9(t0)",
    "ld s10,TF_S10(t0)",
    "ld s11,TF_S11(t0)",
    "ld t3,TF_T3(t0)",
    "ld t4,TF_T4(t0)",
    "ld t5,TF_T5(t0)",
    "ld t6,TF_T6(t0)",

    "ld t1,TF_PC(t0)",
    "csrw sepc,t1",
    "ld t1,TF_SSTATUS(t0)",
    "csrw sstatus,t1",

    "csrw sscratch,t0",

    "ld t1, TF_PROCESS_SATP(t0)",
    "csrw satp, t1",
    "ld t1, TF_IS_LICHEE_RVNANO(t0)",
    "beqz t1, 1f",
    ".long 0x0020000b",
    ".long 0x0190000b",
    "1: sfence.vma zero, zero",

    "ld t1,TF_T1(t0)",
    "ld t0,TF_T0(t0)",

    "sret",
);

#[no_mangle]
#[link_section = ".process_trampoline"]
extern "C" fn trap_handler() -> usize {
    let tf: &mut TrapFrame = unsafe { &mut *(kernel_memory_map::TRAP_FRAME as *mut TrapFrame) };
    let sepc = tf.sepc;
    let scause = tf.scause;
    let stval = tf.stval;

    println!(
        "Trap handler called: scause: {:#x}, stval: {:#x}, sepc: {:#x}",
        scause, stval, sepc
    );
    const USER_ECALL: usize = 8;
    const LOAD_PAGE_FAULT: usize = 13;
    const STORE_PAGE_FAULT: usize = 15;

    match scause & 0xff {
        LOAD_PAGE_FAULT | STORE_PAGE_FAULT => {
            if !kernel_memory_map::grow_stack_on_page_fault(stval) {
                panic!("Unhandled page fault at address {:#x} (scause: {})", stval, scause);
            }
        }
        USER_ECALL => syscall::handle(tf),
        _ => {
            panic!("Unhandled trap: scause: {:#x}, stval: {:#x}", scause, stval);
        }
    }

    tf as *mut TrapFrame as usize
}

