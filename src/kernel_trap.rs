#[repr(align(16))]
#[allow(dead_code)]
struct AlignedStack([u8; 4096]);

#[no_mangle]
static mut KERNEL_STACK: AlignedStack = AlignedStack([0; 4096]);

extern "C" {
    pub fn kernel_trap_entry();
}

core::arch::global_asm!(
    ".section .text.trap_entry",
    ".globl kernel_trap_handler",
    ".type trap_entry_panic, @function",
    "kernel_trap_entry:",
    "la sp, KERNEL_STACK + 4096",
    "call kernel_trap_handler",
);

#[no_mangle]
pub extern "C" fn kernel_trap_handler() {
    let scause: usize;
    let sepc: usize;
    let stval: usize;

    unsafe {
        core::arch::asm!(
            "csrr {0}, scause",
            "csrr {1}, sepc",
            "csrr {2}, stval",
            out(reg) scause,
            out(reg) sepc,
            out(reg) stval,
        );
    }

    println!("*** KERNEL TRAP ***");
    println!("scause = {:#x}", scause);
    println!("sepc   = {:#x}", sepc);
    println!("stval  = {:#x}", stval);

    // Halt the system
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}
