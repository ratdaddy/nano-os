//! Block device dispatcher thread
//!
//! Per-device thread that manages block I/O requests and completions.

use alloc::boxed::Box;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::block::{BlockDevice, BlockError};
use crate::drivers::{sd, virtio_blk};
use crate::dtb;
use crate::thread;

/// Read the RISC-V time register
#[inline]
fn read_time() -> u64 {
    let time: u64;
    unsafe {
        core::arch::asm!("rdtime {}", out(reg) time);
    }
    time
}

static DISPATCHER_TID: AtomicUsize = AtomicUsize::new(0);

/// Message type for block I/O requests and completions
pub enum BlockMessage {
    ReadRequest {
        sector: u32,
        buffer: *mut [u8; 512],
        requester_tid: usize,
    },
    ReadComplete {
        status: Result<(), BlockError>,
    },
    ReadResponse {
        status: Result<(), BlockError>,
    },
}

unsafe impl Send for BlockMessage {}

/// Request a block read from the dispatcher
///
/// The caller must provide a buffer that will be filled with the block data.
/// The buffer must remain valid until the read completes.
pub fn request_read_block(sector: u32, buffer: &mut [u8; 512]) {
    let dispatcher_tid = DISPATCHER_TID.load(Ordering::Relaxed);
    if dispatcher_tid != 0 {
        let requester_tid = thread::Thread::current().id;
        let msg = BlockMessage::ReadRequest {
            sector,
            buffer: buffer as *mut [u8; 512],
            requester_tid,
        };
        let ptr = Box::into_raw(Box::new(msg));
        thread::send_message_urgent(dispatcher_tid, thread::Message {
            sender: requester_tid,
            data: ptr as usize,
        });
    }
}

/// Send a read completion message to the dispatcher
pub fn send_read_completion(status: Result<(), BlockError>) {
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

/// Pending request state
struct PendingRequest {
    requester_tid: usize,
    start_time: u64,
}

static mut PENDING_REQUEST: Option<PendingRequest> = None;

/// Dispatcher thread main loop
fn dispatcher_main() {
    let tid = thread::Thread::current().id;
    kprintln!("Block dispatcher thread started (tid={})", tid);

    // Register our TID so interrupt handlers can send us messages
    DISPATCHER_TID.store(tid, Ordering::Relaxed);

    // Initialize device based on CPU type
    let cpu_type = dtb::get_cpu_type();
    kprintln!("Dispatcher: CPU type = {:?}", cpu_type);

    let mut device: Box<dyn BlockDevice> = match cpu_type {
        dtb::CpuType::Qemu => {
            match virtio_blk::init() {
                Ok(dev) => {
                    kprintln!("Dispatcher: VirtIO device initialized");
                    Box::new(dev)
                }
                Err(e) => {
                    panic!("Dispatcher: Failed to initialize VirtIO: {:?}", e);
                }
            }
        }
        dtb::CpuType::LicheeRVNano => {
            match sd::init() {
                Ok(dev) => {
                    kprintln!("Dispatcher: SD device initialized");
                    Box::new(dev)
                }
                Err(e) => {
                    panic!("Dispatcher: Failed to initialize SD: {:?}", e);
                }
            }
        }
        dtb::CpuType::Unknown => {
            panic!("Dispatcher: Unknown CPU type, cannot proceed");
        }
    };

    kprintln!("Dispatcher: Ready, waiting for read requests...");

    // Main dispatcher loop - handles both VirtIO and SD
    loop {
        let msg = thread::receive_message();
        let block_msg = unsafe { *Box::from_raw(msg.data as *mut BlockMessage) };

        match block_msg {
            BlockMessage::ReadRequest { sector, buffer, requester_tid } => {
                kprintln!("Dispatcher: ReadRequest from tid={}, sector={}",
                         requester_tid, sector);

                // Get caller's buffer
                let buf = unsafe { &mut *buffer };

                // Issue read to device
                let start_time = read_time();
                match device.read_block(sector, buf) {
                    Ok(_) => {
                        // Track pending request
                        unsafe {
                            let ptr = &raw mut PENDING_REQUEST;
                            *ptr = Some(PendingRequest {
                                requester_tid,
                                start_time,
                            });
                        }
                    }
                    Err(e) => {
                        kprintln!("Dispatcher: Failed to issue read: {:?}", e);
                        // Send error response to requester
                        let response = BlockMessage::ReadResponse { status: Err(e) };
                        let ptr = Box::into_raw(Box::new(response));
                        thread::send_message(requester_tid, thread::Message {
                            sender: tid,
                            data: ptr as usize,
                        });
                    }
                }
            }
            BlockMessage::ReadComplete { status } => {
                let end_time = read_time();

                // Get pending request info
                let pending = unsafe {
                    let ptr = &raw mut PENDING_REQUEST;
                    (*ptr).take()
                };

                if let Some(req) = pending {
                    let elapsed_cycles = end_time - req.start_time;
                    let timebase_freq = dtb::get_timebase_frequency();
                    let elapsed_us = (elapsed_cycles * 1_000_000) / timebase_freq;

                    match status {
                        Ok(()) => {
                            kprintln!("Dispatcher: Read completed in {} us", elapsed_us);
                        }
                        Err(e) => {
                            kprintln!("Dispatcher: Read failed: {:?}", e);
                        }
                    }

                    // Send response to requester
                    let response = BlockMessage::ReadResponse { status };
                    let ptr = Box::into_raw(Box::new(response));
                    thread::send_message(req.requester_tid, thread::Message {
                        sender: tid,
                        data: ptr as usize,
                    });
                } else {
                    kprintln!("Dispatcher: ReadComplete but no pending request!");
                }
            }
            BlockMessage::ReadResponse { .. } => {
                // Dispatcher should never receive ReadResponse (it only sends them)
                kprintln!("Dispatcher: ERROR - Received unexpected ReadResponse message!");
            }
        }
    }
}
