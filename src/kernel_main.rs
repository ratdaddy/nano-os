use crate::kernel_trap;

pub fn kernel_main() -> ! {
    println!("In kernel_main");

    unsafe {
        core::arch::asm!(
            "csrw stvec, {}",
            in(reg) kernel_trap::kernel_trap_entry as usize,
        );
    }

    // Uncomment to run demos instead of the init process:
    crate::demos::threading::test_threading();
    // crate::demos::uart::uart_demo();
    // crate::demos::stack::test_stack_allocation();
    // crate::demos::initramfs_inspect::inspect_initramfs(ptr);

    // crate::process_init::run_init_process()
}
