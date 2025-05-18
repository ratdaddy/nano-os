use crate::kernel_main;
use crate::kernel_memory_map::TRAP_FRAME;
use crate::page_mapper;

#[repr(C)]
pub struct ProcessContext {
    pub user_sp: usize,
    pub user_pc: usize,
    pub user_status: usize,
    pub page_map: page_mapper::PageMapper,
}

#[no_mangle]
#[link_section = ".process_trampoline"]
pub unsafe fn enter_process(context: &ProcessContext) -> ! {
    let root_table = context.page_map.root_table;
    println!("Memory mapped at root table: {:#x}", root_table as usize);

    let ppn = root_table as usize >> 12;
    let satp_value = (8 << 60) | ppn;

    println!("Switching to memory map with SATP value: {:#x}", satp_value);
    println!("User stack pointer: {:#x}", context.user_sp);

    core::arch::asm!(
        "la t0, {trap_frame}",     // Load address of TRAP_FRAME into t0
        "ld t0, 0(t0)",
        "csrw sscratch, t0",       // Write t0 into sscratch
        "csrw stvec, {trap_entry}",
        "csrw satp, {satp_value}",
        "ld t0, 288(t0)",
        "beqz t0, 1f",
        ".long 0x0020000b",
        ".long 0x0190000b",
     "1: sfence.vma zero, zero",
        "csrw sepc, {user_pc}",        // user entry point
        //"csrw sstatus, {?}",     // user-mode status (e.g., SPIE = 1, SPP = 0)
        "mv sp, {user_sp}",
        "sret",

        trap_entry = in(reg) kernel_main::trap_entry as usize,
        satp_value = in(reg) satp_value,
        user_pc = in(reg) context.user_pc,
        user_sp = in(reg) context.user_sp,
        trap_frame = sym TRAP_FRAME,

        options(nostack)
    );

    loop {
        unsafe { core::arch::asm!("wfi") }
    }
}
