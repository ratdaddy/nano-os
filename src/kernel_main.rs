use crate::console;
use crate::drivers::{plic, uart};
use crate::initramfs;
use crate::kernel_trap;
use crate::kthread;

pub fn kernel_main() -> ! {
    println!("In kernel_main");

    unsafe {
        core::arch::asm!(
            "csrw stvec, {}",
            in(reg) kernel_trap::kernel_trap_entry as usize,
        );
    }

    // Initialize PLIC and UART
    unsafe { plic::init(); }
    uart::init();

    // Mount initramfs (available for all demos and process loading)
    initramfs::init();

    kthread::uart_writer::init();
    kthread::idle::init();

    loop {
        println!();
        println!("=== Nano OS Boot Menu ===");
        println!("1) Thread message passing demo");
        println!("2) UART interrupt demo");
        println!("3) Initramfs inspect");
        println!("4) ELF inspect");
        println!("5) Run init process");
        println!("6) UART TX flood test");
        print!("Select: ");

        let ch = console::getchar();
        console::sbi_console_putchar(ch); // echo
        println!();

        match ch {
            b'1' => crate::demos::threading::test_message_passing(),
            b'2' => crate::demos::uart::uart_demo(),
            b'3' => crate::demos::initramfs_inspect::inspect_initramfs(),
            b'4' => crate::demos::elf_inspect::inspect_elf(),
            b'5' => crate::process_init::run_init_process(),
            b'6' => crate::demos::uart_flood::run(),
            _ => println!("Invalid selection"),
        }
    }
}
