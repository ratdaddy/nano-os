use crate::kernel_memory_map;

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

    "ld t0, 280(sp)",
    "csrw satp, t0",

    "ld t0, 288(sp)",
    "beqz t0, 1f",

    ".long 0x0020000b",
    ".long 0x0190000b",

 "1: sfence.vma zero, zero",
    "ld sp, KERNEL_STACK_START",

    "csrr a0, scause",
    "csrr a1, stval",
    "call trap_handler",

    /*
    // Restore registers
    "ld a0, 0(sp)",
    "ld a1, 8(sp)",
    ...
    "addi sp, sp, 16",
    // Restore original sp
    "csrrw sp, sscratch, t1",
    "sret",
     */
);

#[no_mangle]
#[link_section = ".process_trampoline"]
extern "C" fn trap_handler(scause: usize, stval: usize) {
    let sepc: usize;
    unsafe {
        core::arch::asm!("csrr {0}, sepc", out(reg) sepc);
    }

    println!("Trap handler called: scause: {:#x}, stval: {:#x}, sepc: {:#x}", scause, stval, sepc);
    const USER_ECALL: usize = 8;
    const LOAD_PAGE_FAULT: usize = 13;
    const STORE_PAGE_FAULT: usize = 15;

    match scause & 0xff {
        LOAD_PAGE_FAULT | STORE_PAGE_FAULT => {
            if !kernel_memory_map::grow_stack_on_page_fault(stval) {
                panic!("Unhandled page fault at address {:#x} (scause: {})", stval, scause);
            }
        }
        USER_ECALL => {}
        _ => {
            panic!("Unhandled trap: scause: {:#x}, stval: {:#x}", scause, stval);
        }
    }

    loop {
        unsafe { core::arch::asm!("wfi") }
    }
}

#[repr(C)]
pub struct TrapFrame {
    pub ra: usize,
    pub sp: usize,
    pub gp: usize,
    pub tp: usize,
    pub t0: usize,
    pub t1: usize,
    pub t2: usize,
    pub s0: usize,
    pub s1: usize,
    pub a0: usize,
    pub a1: usize,
    pub a2: usize,
    pub a3: usize,
    pub a4: usize,
    pub a5: usize,
    pub a6: usize,
    pub a7: usize,
    pub s2: usize,
    pub s3: usize,
    pub s4: usize,
    pub s5: usize,
    pub s6: usize,
    pub s7: usize,
    pub s8: usize,
    pub s9: usize,
    pub s10: usize,
    pub s11: usize,
    pub t3: usize,
    pub t4: usize,
    pub t5: usize,
    pub t6: usize, // 240
    pub sepc: usize, // 248
    pub sstatus: usize, // 256
    pub stval: usize, // 264
    pub scause: usize, // 272
    pub kernel_satp: usize, // 280
    pub is_lichee_rvnano: usize, // 288
}
