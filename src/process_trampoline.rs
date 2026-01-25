use crate::kernel_memory_map::TRAMPOLINE_TRAP_FRAME;
use crate::plic;
use crate::process;
use crate::trap;

#[no_mangle]
#[link_section = ".process_trampoline"]
pub unsafe fn enter_process(context: &mut process::Context) -> ! {
    process::Context::set_current(context);
    println!("Switching to memory map with SATP value: {:#x}", context.satp);
    println!("User stack pointer: {:#x}", context.trap_frame.registers.sp);
    println!("User program counter: {:#x}", context.trap_frame.pc);

    let tramp_trap_frame = TRAMPOLINE_TRAP_FRAME as *mut types::TrampolineTrapFrame;
    (*tramp_trap_frame).kernel_sp = context.trap_frame as *mut _ as usize;
    context.trap_frame.satp = context.satp;

    println!("Enabling interrupts");
    unsafe {
        plic::init();
    }

    println!("Entering user process with trap frame at: {:#x}", &context.trap_frame as *const _ as usize);

    core::arch::asm!(
        // load address of TRAP_FRAME into sscratch
        "la t0, {trap_frame}",
        "ld t0, 0(t0)",
        "csrw sscratch, t0",
        // Enable interrupts
        "li t1, 0x202",
        "csrw sie, t1",
        // Set sstatus
        "csrr t1, sstatus",
        "li t2, (1 << 5)",          // SPIE
        "or t1, t1, t2",
        "li t2, ~(1 << 8)",         // Clear SPP (bit 8)
        "and t1, t1, t2",
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
        "csrw sepc, {user_pc}",        // user entry point
        //"csrw sstatus, {?}",     // user-mode status (e.g., SPIE = 1, SPP = 0)
        "mv sp, {user_sp}",
        "sret",

        trap_entry = in(reg) trap::trap_entry as usize,
        satp_value = in(reg) context.satp,
        user_pc = in(reg) context.trap_frame.pc,
        user_sp = in(reg) context.trap_frame.registers.sp,
        trap_frame = sym TRAMPOLINE_TRAP_FRAME,

        options(nostack)
    );

    loop {
        unsafe { core::arch::asm!("wfi") }
    }
}
