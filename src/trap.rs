use crate::kernel_memory_map;
use crate::cpu;
use crate::syscall;

core::arch::global_asm!(
    ".section .process_trampoline",
    ".global trap_entry",
    "trap_entry:",
    "csrrw sp, sscratch, sp",
    //Save caller-saved registers
    "sd ra,  0(sp)",
    "sd gp, 16(sp)",
    "sd tp, 24(sp)",
    "sd t0, 32(sp)",
    "csrr t0, sscratch",
    "sd t0, 8(sp)",
    "sd t1, 40(sp)",
    "sd t2, 48(sp)",
    "sd s0, 56(sp)",
    "sd s1, 64(sp)",
    "sd a0, 72(sp)",
    "sd a1, 80(sp)",
    "sd a2, 88(sp)",
    "sd a3, 96(sp)",
    "sd a4, 104(sp)",
    "sd a5, 112(sp)",
    "sd a6, 120(sp)",
    "sd a7, 128(sp)",
    "sd s2, 136(sp)",
    "sd s3, 144(sp)",
    "sd s4, 152(sp)",
    "sd s5, 160(sp)",
    "sd s6, 168(sp)",
    "sd s7, 176(sp)",
    "sd s8, 184(sp)",
    "sd s9, 192(sp)",
    "sd s10, 200(sp)",
    "sd s11, 208(sp)",
    "sd t3, 216(sp)",
    "sd t4, 224(sp)",
    "sd t5, 232(sp)",
    "sd t6, 240(sp)",
    "csrr t0, sepc",
    "sd t0, 248(sp)",
    "sd t0, 256(sp)",
    "csrr t0, sstatus",
    "sd t0, 264(sp)",
    "csrr t0, stval",
    "sd t0, 272(sp)",
    "csrr t0, scause",
    "sd t0, 280(sp)",
    "ld t0, 288(sp)",
    "csrw satp, t0",
    "ld t0, 304(sp)",
    "beqz t0, 1f",
    ".long 0x0020000b",
    ".long 0x0190000b",
    "1: sfence.vma zero, zero",
    "ld sp, KERNEL_STACK_START",

    // Handle trap
    "call trap_handler",

    // Restore registers
    "mv t0, a0",

    "ld ra,0(t0)",
    "ld sp,8(t0)",
    "ld gp,16(t0)",
    "ld tp,24(t0)",
    // skip t0 & t1 for now
    "ld t2,48(t0)",
    "ld s0,56(t0)",
    "ld s1,64(t0)",
    "ld a0,72(t0)",
    "ld a1,80(t0)",
    "ld a2,88(t0)",
    "ld a3,96(t0)",
    "ld a4,104(t0)",
    "ld a5,112(t0)",
    "ld a6,120(t0)",
    "ld a7,128(t0)",
    "ld s2,136(t0)",
    "ld s3,144(t0)",
    "ld s4,152(t0)",
    "ld s5,160(t0)",
    "ld s6,168(t0)",
    "ld s7,176(t0)",
    "ld s8,184(t0)",
    "ld s9,192(t0)",
    "ld s10,200(t0)",
    "ld s11,208(t0)",
    "ld t3,216(t0)",
    "ld t4,224(t0)",
    "ld t5,232(t0)",
    "ld t6,240(t0)",

    "ld t1,248(t0)",
    "csrw sepc,t1",
    "ld t1,264(t0)",
    "csrw sstatus,t1",

    "csrw sscratch,t0",

    "ld t1, 296(t0)",
    "csrw satp, t1",
    "ld t1, 304(t0)",
    "beqz t1, 1f",
    ".long 0x0020000b",
    ".long 0x0190000b",
    "1: sfence.vma zero, zero",

    "ld t1,40(t0)",
    "ld t0,32(t0)",

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

#[repr(C)]
pub struct TrapFrame {
    pub registers: cpu::Registers,
    pub sepc: usize,             // 256
    pub sstatus: usize,          // 264
    pub stval: usize,            // 272
    pub scause: usize,           // 280
    pub kernel_satp: usize,      // 288
    pub process_satp: usize,     // 296
    pub is_lichee_rvnano: usize, // 304
}
