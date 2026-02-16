use alloc::boxed::Box;

use crate::block::disk::{self, BlockMessage};
use crate::block::{partition, BlockDisk};
use crate::console;
use crate::drivers::{plic, sd, uart, virtio_blk};
use crate::dtb;
use crate::initramfs;
use crate::kernel_trap;
use crate::kprint;
use crate::kthread;
use crate::thread;
use crate::vfs;
use crate::{demos, procfs, ramfs};

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

    // Register filesystem drivers
    vfs::register_filesystem(&ramfs::RAMFS_TYPE);
    vfs::register_filesystem(&procfs::PROCFS_TYPE);

    // Mount initramfs as root filesystem
    vfs::init(initramfs::new());
    vfs::vfs_mount_at("/proc", "proc").expect("failed to mount /proc");

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
        println!("    6) Mount table");
        println!("    7) Filesystem contents");
        println!("    8) ELF headers");
        println!("    9) Procfs contents");
        println!();
        println!("  Hardware:");
        println!("    s) SD card (PIO mode) (NanoRV)");
        println!("    a) SD card (ADMA2 mode) (NanoRV)");
        println!("    v) virtio-blk device (QEMU)");
        println!();
        println!("  Block Layer:");
        println!("    d) Start block dispatcher thread");
        println!();
        print!("Select: ");

        let ch = console::getchar();
        console::sbi_console_putchar(ch); // echo
        println!();

        match ch {
            b'1' => run_process_as_kthread(),
            b'2' => run_two_processes(),
            b'3' => demos::threading::test_message_passing(),
            b'4' => demos::uart::uart_demo(),
            b'5' => demos::uart_flood::run(),
            b'6' => demos::mount_inspect::inspect_mounts(),
            b'7' => demos::vfs_inspect::inspect_vfs(),
            b'8' => demos::elf_inspect::inspect_elf(),
            b'9' => demos::procfs_inspect::inspect_procfs(),
            b's' => demos::sd_read::sd_read_demo(),
            b'a' => demos::sd_adma::sd_adma_demo(),
            b'v' => demos::virtio_blk::virtio_blk_demo(),
            b'd' => spawn_block_dispatcher(),
            _ => println!("Invalid selection"),
        }

        // Wait for keypress before redisplaying menu
        // (menu items that don't return, like options 1 & 2, never reach here)
        println!();
        print!("Press any key to continue...");
        console::getchar();
        println!();
    }
}

/// Spawn the init process as a kernel thread and start the scheduler.
fn run_process_as_kthread() -> ! {
    match kthread::user_process::spawn_process("/prog_example") {
        Ok(_tid) => {
            #[cfg(feature = "trace_process")]
            println!("Process spawned as thread {}, starting scheduler...", _tid);
        }
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
        Ok(_tid) => {
            #[cfg(feature = "trace_process")]
            println!("Process 1 spawned as thread {}", _tid);
        }
        Err(e) => {
            println!("Failed to spawn process 1: {}", e);
            loop { unsafe { core::arch::asm!("wfi"); } }
        }
    }

    match kthread::user_process::spawn_process("/prog_example") {
        Ok(_tid) => {
            #[cfg(feature = "trace_process")]
            println!("Process 2 spawned as thread {}", _tid);
        }
        Err(e) => {
            println!("Failed to spawn process 2: {}", e);
            loop { unsafe { core::arch::asm!("wfi"); } }
        }
    }

    #[cfg(feature = "trace_process")]
    println!("Starting scheduler with two processes...");
    thread::start_scheduler()
}

/// Spawn the block dispatcher thread and a reader thread
fn spawn_block_dispatcher() {
    // Probe hardware and initialize appropriate driver
    let disk = match dtb::get_cpu_type() {
        dtb::CpuType::Qemu => {
            let driver = virtio_blk::init().expect("VirtIO init");
            BlockDisk::new(driver)
        }
        dtb::CpuType::LicheeRVNano => {
            let driver = sd::init().expect("SD init");
            BlockDisk::new(driver)
        }
        _ => panic!("Unknown CPU type"),
    }.expect("Failed to create BlockDisk");

    println!("Block dispatcher started (tid={})", disk.dispatcher_tid());

    // Spawn a reader thread that will make block requests
    match spawn_block_reader() {
        Ok(tid) => {
            println!("Block reader spawned as thread {}", tid);
        }
        Err(e) => {
            println!("Failed to spawn block reader: {}", e);
            loop { unsafe { core::arch::asm!("wfi"); } }
        }
    }

    thread::start_scheduler();

    // Fallback wfi loop - should never reach here since scheduler never returns
    #[allow(unreachable_code)]
    loop { unsafe { core::arch::asm!("wfi"); } }
}

/// Spawn a block reader thread
fn spawn_block_reader() -> Result<usize, &'static str> {
    let t = thread::Thread::new(block_reader_main);
    let tid = t.id;
    thread::add(t);
    Ok(tid)
}

// Static buffer for block reader - must be properly aligned for DMA
#[repr(C, align(512))]
struct ReaderBuffer([u8; 512]);

static mut READER_BUFFER: ReaderBuffer = ReaderBuffer([0; 512]);

/// Block reader thread main loop
fn block_reader_main() {
    let tid = thread::Thread::current().id;
    kprintln!("Block reader thread started (tid={})", tid);

    unsafe {
        let buf = &raw mut READER_BUFFER.0;
        let buf = &mut *buf;

        kprintln!("Block reader: Requesting read of sector 0...");

        // Request block read from dispatcher
        disk::request_read_block(0, buf);

        kprintln!("Block reader: Waiting for completion...");

        // Wait for response message from dispatcher
        let msg = thread::receive_message();
        let response = *Box::from_raw(msg.data as *mut BlockMessage);

        // Check response status
        if let BlockMessage::ReadResponse { status } = response {
            match status {
                Ok(()) => {
                    kprintln!("Block reader: Read completed successfully!");

                    // Parse and display partition table
                    partition::parse_mbr(buf);
                }
                Err(e) => {
                    kprintln!("Block reader: Read failed: {:?}", e);
                }
            }
        } else {
            kprintln!("Block reader: Unexpected message type!");
        }
    }

    kprintln!("Block reader: Done, exiting...");
    thread::exit();
}
