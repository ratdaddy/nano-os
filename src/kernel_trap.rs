use crate::drivers::plic;
use crate::riscv;

#[repr(align(16))]
#[allow(dead_code)]
struct AlignedStack([u8; 8192]);

#[no_mangle]
static mut KERNEL_STACK: AlignedStack = AlignedStack([0; 8192]);

/// Returns the top of the kernel trap stack.
/// Used by the idle thread to set sscratch before enabling interrupts.
pub fn trap_stack_top() -> usize {
    unsafe { core::ptr::addr_of!(KERNEL_STACK).add(1) as usize }
}

extern "C" {
    pub fn kernel_trap_entry();
}

// Kernel trap entry: saves ALL registers (caller + callee saved), calls handler, restores, sret.
//
// Since traps can occur at any point during kernel execution, we must preserve
// all registers - the interrupted code may be using callee-saved registers.
//
// On entry, sscratch holds the kernel trap stack top.
// We swap sp with sscratch to get the trap stack and save the original sp.
//
// Frame layout matches KernelTrapFrame (256 bytes, 16-byte aligned):
//   - Offsets 0-247: all 31 GP registers (GpRegisters layout)
//   - Offset 248: sepc
//
// Offsets are generated in build.rs as KTF_* constants.
core::arch::global_asm!(
    include_str!(concat!(env!("OUT_DIR"), "/offsets.S")),

    ".section .text.trap_entry",
    ".globl kernel_trap_entry",
    "kernel_trap_entry:",

    // Swap sp with sscratch (sp = trap stack, sscratch = interrupted sp)
    "csrrw sp, sscratch, sp",

    // Allocate frame
    "addi sp, sp, -KTF_SIZE",

    // Save all general-purpose registers (except sp, handled separately)
    "sd ra,  KTF_RA(sp)",
    "sd gp,  KTF_GP(sp)",
    "sd tp,  KTF_TP(sp)",
    "sd t0,  KTF_T0(sp)",
    "sd t1,  KTF_T1(sp)",
    "sd t2,  KTF_T2(sp)",
    "sd s0,  KTF_S0(sp)",
    "sd s1,  KTF_S1(sp)",
    "sd a0,  KTF_A0(sp)",
    "sd a1,  KTF_A1(sp)",
    "sd a2,  KTF_A2(sp)",
    "sd a3,  KTF_A3(sp)",
    "sd a4,  KTF_A4(sp)",
    "sd a5,  KTF_A5(sp)",
    "sd a6,  KTF_A6(sp)",
    "sd a7,  KTF_A7(sp)",
    "sd s2,  KTF_S2(sp)",
    "sd s3,  KTF_S3(sp)",
    "sd s4,  KTF_S4(sp)",
    "sd s5,  KTF_S5(sp)",
    "sd s6,  KTF_S6(sp)",
    "sd s7,  KTF_S7(sp)",
    "sd s8,  KTF_S8(sp)",
    "sd s9,  KTF_S9(sp)",
    "sd s10, KTF_S10(sp)",
    "sd s11, KTF_S11(sp)",
    "sd t3,  KTF_T3(sp)",
    "sd t4,  KTF_T4(sp)",
    "sd t5,  KTF_T5(sp)",
    "sd t6,  KTF_T6(sp)",

    // Save original sp (from sscratch) - this is the interrupted code's sp
    "csrr t0, sscratch",
    "sd t0, KTF_SP(sp)",

    // Save sepc (needed for sret to return to the right place)
    "csrr t0, sepc",
    "sd t0, KTF_SEPC(sp)",

    // Call Rust handler
    "call kernel_trap_handler",

    // Restore sepc
    "ld t0, KTF_SEPC(sp)",
    "csrw sepc, t0",

    // Load original sp into sscratch (will be swapped back at the end)
    "ld t0, KTF_SP(sp)",
    "csrw sscratch, t0",

    // Restore all general-purpose registers (except sp)
    "ld ra,  KTF_RA(sp)",
    "ld gp,  KTF_GP(sp)",
    "ld tp,  KTF_TP(sp)",
    "ld t0,  KTF_T0(sp)",
    "ld t1,  KTF_T1(sp)",
    "ld t2,  KTF_T2(sp)",
    "ld s0,  KTF_S0(sp)",
    "ld s1,  KTF_S1(sp)",
    "ld a0,  KTF_A0(sp)",
    "ld a1,  KTF_A1(sp)",
    "ld a2,  KTF_A2(sp)",
    "ld a3,  KTF_A3(sp)",
    "ld a4,  KTF_A4(sp)",
    "ld a5,  KTF_A5(sp)",
    "ld a6,  KTF_A6(sp)",
    "ld a7,  KTF_A7(sp)",
    "ld s2,  KTF_S2(sp)",
    "ld s3,  KTF_S3(sp)",
    "ld s4,  KTF_S4(sp)",
    "ld s5,  KTF_S5(sp)",
    "ld s6,  KTF_S6(sp)",
    "ld s7,  KTF_S7(sp)",
    "ld s8,  KTF_S8(sp)",
    "ld s9,  KTF_S9(sp)",
    "ld s10, KTF_S10(sp)",
    "ld s11, KTF_S11(sp)",
    "ld t3,  KTF_T3(sp)",
    "ld t4,  KTF_T4(sp)",
    "ld t5,  KTF_T5(sp)",
    "ld t6,  KTF_T6(sp)",

    // Deallocate frame
    "addi sp, sp, KTF_SIZE",

    // Restore original sp from sscratch, put trap stack top back in sscratch
    "csrrw sp, sscratch, sp",

    "sret",
);

#[no_mangle]
pub extern "C" fn kernel_trap_handler() {
    let scause: usize;
    let sepc: usize;

    unsafe {
        core::arch::asm!(
            "csrr {0}, scause",
            "csrr {1}, sepc",
            out(reg) scause,
            out(reg) sepc,
        );
    }

    if riscv::is_interrupt(scause) {
        let cause = riscv::interrupt::code(scause);
        match cause {
            riscv::interrupt::code::EXTERNAL => {
                plic::dispatch_irq();
            }
            _ => {
                panic!("Unexpected interrupt: scause={:#x}", scause);
            }
        }
    } else {
        // Exception — fatal error
        let stval: usize;
        unsafe {
            core::arch::asm!("csrr {}, stval", out(reg) stval);
        }

        panic!(
            "KERNEL TRAP: scause={:#x} sepc={:#x} stval={:#x}",
            scause, sepc, stval
        );
    }
}
