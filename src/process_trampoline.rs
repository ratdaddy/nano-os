use crate::kernel_main;
use crate::kernel_memory_map::TRAP_FRAME;
use crate::process;

#[no_mangle]
#[link_section = ".process_trampoline"]
pub unsafe fn enter_process(context: &process::Context) -> ! {
    println!("Switching to memory map with SATP value: {:#x}", context.satp);
    println!("User stack pointer: {:#x}", context.registers.sp);
    println!("User program counter: {:#x}", context.registers.pc);

    core::arch::asm!(
        "la t0, {trap_frame}",     // Load address of TRAP_FRAME into t0
        "ld t0, 0(t0)",
        "csrw sscratch, t0",       // Write t0 into sscratch
        "csrw stvec, {trap_entry}",
        "csrw satp, {satp_value}",
        "ld t0, 304(t0)",
        "beqz t0, 1f",
        ".long 0x0020000b",
        ".long 0x0190000b",
     "1: sfence.vma zero, zero",
        "csrw sepc, {user_pc}",        // user entry point
        //"csrw sstatus, {?}",     // user-mode status (e.g., SPIE = 1, SPP = 0)
        "mv sp, {user_sp}",
        "sret",

        trap_entry = in(reg) kernel_main::trap_entry as usize,
        satp_value = in(reg) context.satp,
        user_pc = in(reg) context.registers.pc,
        user_sp = in(reg) context.registers.sp,
        trap_frame = sym TRAP_FRAME,

        options(nostack)
    );

    loop {
        unsafe { core::arch::asm!("wfi") }
    }
}
