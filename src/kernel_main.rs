use crate::console;
use crate::kernel_trap;

pub fn kernel_main() -> ! {
    println!("In kernel_main");

    unsafe {
        core::arch::asm!(
            "csrw stvec, {}",
            in(reg) kernel_trap::kernel_trap_entry as usize,
        );
    }

    loop {
        println!();
        println!("=== Nano OS Boot Menu ===");
        println!("1) Thread message passing demo");
        println!("2) UART interrupt demo");
        println!("3) Initramfs inspect");
        println!("4) Run init process");
        print!("Select: ");

        let ch = console::getchar();
        console::sbi_console_putchar(ch); // echo
        println!();

        match ch {
            b'1' => crate::demos::threading::test_threading(),
            b'2' => crate::demos::uart::uart_demo(),
            b'3' => crate::demos::initramfs_inspect::inspect_initramfs(),
            b'4' => crate::process_init::run_init_process(),
            _ => println!("Invalid selection"),
        }
    }
}
