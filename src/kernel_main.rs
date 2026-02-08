use crate::console;
use crate::drivers::{plic, uart};
use crate::initramfs;
use crate::kernel_trap;
use crate::kprint;
use crate::kthread;
use crate::thread;
use crate::vfs;

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

    // Mount initramfs as root filesystem
    vfs::init(initramfs::new());

    uart::register_chrdev();
    kthread::uart_writer::init();
    kprint::init();
    kthread::idle::init();

    loop {
        println!();
        println!("=== Nano OS Boot Menu ===");
        println!();
        println!("  Process:");
        println!("    1) Run one process");
        println!("    2) Run two processes");
        println!();
        println!("  Demos:");
        println!("    3) Thread message passing");
        println!("    4) UART RX interrupts");
        println!("    5) UART TX flood");
        println!();
        println!("  Inspect:");
        println!("    6) VFS");
        println!("    7) ELF headers");
        println!();
        print!("Select: ");

        let ch = console::getchar();
        console::sbi_console_putchar(ch); // echo
        println!();

        match ch {
            b'1' => run_process_as_kthread(),
            b'2' => run_two_processes(),
            b'3' => crate::demos::threading::test_message_passing(),
            b'4' => crate::demos::uart::uart_demo(),
            b'5' => crate::demos::uart_flood::run(),
            b'6' => crate::demos::vfs_inspect::inspect_vfs(),
            b'7' => crate::demos::elf_inspect::inspect_elf(),
            _ => println!("Invalid selection"),
        }
    }
}

/// Spawn the init process as a kernel thread and start the scheduler.
fn run_process_as_kthread() -> ! {
    match kthread::user_process::spawn_process("/prog_example") {
        Ok(tid) => println!("Process spawned as thread {}, starting scheduler...", tid),
        Err(e) => {
            println!("Failed to spawn process: {}", e);
            loop { unsafe { core::arch::asm!("wfi"); } }
        }
    }

    thread::start_scheduler()
}

/// Spawn two processes to test multi-process scheduling with yield.
fn run_two_processes() -> ! {
    match kthread::user_process::spawn_process("/prog_example") {
        Ok(tid) => println!("Process 1 spawned as thread {}", tid),
        Err(e) => {
            println!("Failed to spawn process 1: {}", e);
            loop { unsafe { core::arch::asm!("wfi"); } }
        }
    }

    match kthread::user_process::spawn_process("/prog_example") {
        Ok(tid) => println!("Process 2 spawned as thread {}", tid),
        Err(e) => {
            println!("Failed to spawn process 2: {}", e);
            loop { unsafe { core::arch::asm!("wfi"); } }
        }
    }

    println!("Starting scheduler with two processes...");
    thread::start_scheduler()
}
