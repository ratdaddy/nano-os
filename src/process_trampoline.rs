use crate::kernel_memory_map::TRAMPOLINE_TRAP_FRAME;
use crate::process;
use crate::riscv;
use crate::trap;

#[no_mangle]
#[link_section = ".process_trampoline"]
pub unsafe fn enter_process(context: &mut process::Context) -> ! {
    process::Context::set_current(context);
    #[cfg(feature = "trace_process")]
    {
        println!("Switching to memory map with SATP value: {:#x}", context.satp);
        println!("User stack pointer: {:#x}", context.trap_frame.registers.sp);
        println!("User program counter: {:#x}", context.trap_frame.pc);
    }

    let tramp_trap_frame = TRAMPOLINE_TRAP_FRAME as *mut types::TrampolineTrapFrame;
    (*tramp_trap_frame).kernel_sp = context.trap_frame as *mut _ as usize;
    context.trap_frame.satp = context.satp;

    #[cfg(feature = "trace_process")]
    println!("Entering user process with trap frame at: {:#x}", &context.trap_frame as *const _ as usize);

    // Enable software and external interrupts (not timer) for user mode
    let sie_value = riscv::SIE_SSIE | riscv::SIE_SEIE;
    let sstatus_spie = riscv::SSTATUS_SPIE;
    let sstatus_spp_mask = !riscv::SSTATUS_SPP;

    core::arch::asm!(
        // load address of TRAP_FRAME into sscratch
        "la t0, {trap_frame}",
        "ld t0, 0(t0)",
        "csrw sscratch, t0",
        // Enable interrupts in sie
        "csrw sie, {sie_value}",
        // Set sstatus: SPIE=1, SPP=0 (return to user mode with interrupts enabled)
        "csrr t1, sstatus",
        "or t1, t1, {sstatus_spie}",
        "and t1, t1, {sstatus_spp_mask}",
        "csrw sstatus, t1",
        // set up the trap handler and user mmap
        "csrw stvec, {trap_entry}",
        "csrw satp, {satp_value}",
        // T-Head C906: flush caches for SATP switch (kernel -> user)
        // See notes/thead-c906-memory-guide.md for cache instruction details
        "ld t0, TTF_IS_LICHEE_RVNANO(t0)",
        "beqz t0, 1f",
        ".long 0x0030000b",   // th.dcache.ciall - clean and invalidate D-cache
        ".long 0x0100000b",   // th.icache.iall  - invalidate I-cache
     "1: sfence.vma zero, zero",
        "csrw sepc, {user_pc}",
        "mv sp, {user_sp}",
        "sret",

        trap_entry = in(reg) trap::trap_entry as usize,
        satp_value = in(reg) context.satp,
        user_pc = in(reg) context.trap_frame.pc,
        user_sp = in(reg) context.trap_frame.registers.sp,
        trap_frame = sym TRAMPOLINE_TRAP_FRAME,
        sie_value = in(reg) sie_value,
        sstatus_spie = in(reg) sstatus_spie,
        sstatus_spp_mask = in(reg) sstatus_spp_mask,
        // t0, t1, t2 are used manually in the asm
        out("t0") _,
        out("t1") _,

        options(nostack)
    );

    loop {
        unsafe { core::arch::asm!("wfi") }
    }
}
