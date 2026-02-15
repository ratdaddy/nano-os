//! Block device dispatcher thread
//!
//! Per-device thread that manages block I/O requests and completions.

use crate::dtb;
use crate::println;
use crate::thread;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Read the RISC-V time register
#[inline]
fn read_time() -> u64 {
    let time: u64;
    unsafe {
        core::arch::asm!("rdtime {}", out(reg) time);
    }
    time
}

// Static buffer for block I/O - must be properly aligned for DMA
#[repr(C, align(512))]
struct BlockBuffer([u8; 512]);

static mut READ_BUFFER: BlockBuffer = BlockBuffer([0; 512]);
static mut START_TIME: u64 = 0;

static DISPATCHER_TID: AtomicUsize = AtomicUsize::new(0);

/// Message type for block completion notifications
pub enum BlockMessage {
    ReadComplete { status: Result<(), crate::block::BlockError> },
}

/// Send a read completion message to the dispatcher
pub fn send_read_completion(status: Result<(), crate::block::BlockError>) {
    let tid = DISPATCHER_TID.load(Ordering::Relaxed);
    if tid != 0 {
        let msg = BlockMessage::ReadComplete { status };
        let ptr = Box::into_raw(Box::new(msg));
        thread::send_message(tid, thread::Message {
            sender: 0,
            data: ptr as usize,
        });
    }
}

/// Spawn the block dispatcher thread for the current platform
pub fn spawn_dispatcher() -> Result<usize, &'static str> {
    let t = thread::Thread::new(dispatcher_main);
    let tid = t.id;
    thread::add(t);
    Ok(tid)
}

/// Dispatcher thread main loop
fn dispatcher_main() {
    use crate::block::BlockDevice;

    let tid = thread::Thread::current().id;
    println!("Block dispatcher thread started (tid={})", tid);

    // Register our TID so interrupt handlers can send us messages
    DISPATCHER_TID.store(tid, Ordering::Relaxed);

    // Select appropriate device based on CPU type
    let cpu_type = dtb::get_cpu_type();
    println!("Dispatcher: CPU type = {:?}", cpu_type);

    match cpu_type {
        dtb::CpuType::Qemu => {
            println!("Dispatcher: Initializing VirtIO block device");

            let mut device = match crate::drivers::virtio_blk::init() {
                Ok(dev) => {
                    println!("Dispatcher: VirtIO device initialized");
                    dev
                }
                Err(e) => {
                    panic!("Dispatcher: Failed to initialize VirtIO: {:?}", e);
                }
            };

            println!("Dispatcher: Starting read of sector 0...");
            let buf = unsafe { &mut *core::ptr::addr_of_mut!(READ_BUFFER.0) };

            let start_time = read_time();
            match device.read_block(0, buf) {
                Ok(_) => println!("Dispatcher: Read request issued, waiting for interrupt..."),
                Err(e) => {
                    println!("Dispatcher: Failed to issue read: {:?}", e);
                    return;
                }
            }

            // Store start time for later
            unsafe { START_TIME = start_time; }
        }
        dtb::CpuType::LicheeRVNano => {
            println!("Dispatcher: Initializing SD block device");

            let mut device = match crate::drivers::sd::init() {
                Ok(dev) => {
                    println!("Dispatcher: SD device initialized");
                    dev
                }
                Err(e) => {
                    panic!("Dispatcher: Failed to initialize SD: {:?}", e);
                }
            };

            println!("Dispatcher: Starting read of sector 0...");
            let buf = unsafe { &mut *core::ptr::addr_of_mut!(READ_BUFFER.0) };

            let start_time = read_time();
            match device.read_block(0, buf) {
                Ok(_) => println!("Dispatcher: Read request issued, waiting for interrupt..."),
                Err(e) => {
                    println!("Dispatcher: Failed to issue read: {:?}", e);
                    return;
                }
            }

            // Store start time for later
            unsafe { START_TIME = start_time; }
        }
        dtb::CpuType::Unknown => {
            println!("Dispatcher: Unknown CPU type, cannot proceed");
            return;
        }
    }

    println!("Dispatcher: Entering main loop, waiting for completion messages...");

    // Main dispatcher loop
    loop {
        // Wait for completion message from interrupt handler
        let msg = thread::receive_message();
        let block_msg = unsafe { *Box::from_raw(msg.data as *mut BlockMessage) };

        match block_msg {
            BlockMessage::ReadComplete { status } => {
                let end_time = read_time();
                let start_time = unsafe { START_TIME };
                let elapsed_cycles = end_time - start_time;

                // Get timebase frequency from DTB
                let timebase_freq = dtb::get_timebase_frequency();
                let elapsed_us = (elapsed_cycles * 1_000_000) / timebase_freq;

                match status {
                    Ok(()) => {
                        println!("Dispatcher: Block read completed successfully");
                        println!("Dispatcher: Elapsed time: {} cycles ({} us)",
                                 elapsed_cycles, elapsed_us);

                        // Parse and display partition table from block 0
                        let buf = unsafe { &*core::ptr::addr_of!(READ_BUFFER.0) };
                        crate::block::partition::parse_mbr(buf);
                    }
                    Err(e) => {
                        println!("Dispatcher: Block read failed: {:?}", e);
                    }
                }
            }
        }

        // TODO: Wake waiting thread(s)
        // TODO: Process next request from queue
    }
}
