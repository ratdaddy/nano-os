//! Block device dispatcher thread
//!
//! Per-device thread that manages block I/O requests and completions.
//!
//! ## Architecture
//!
//! Each BlockDisk has a dedicated dispatcher thread that:
//! - Owns the BlockDriver (hardware driver)
//! - Serializes I/O requests from multiple readers
//! - Handles asynchronous hardware operations
//! - Tracks timing and performance metrics
//!
//! ## Thread Initialization
//!
//! Since `Thread::new()` only accepts `fn()` pointers (not closures), we cannot
//! directly pass the driver to the new thread. Instead, we use a temporary handoff
//! via `PENDING_DRIVER`: the spawning code stores the driver there, creates the
//! thread, and the new thread immediately takes ownership.

use alloc::boxed::Box;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

use crate::drivers::{BlockDriver, BlockError};
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

/// Temporary storage for passing driver to dispatcher thread.
///
/// The spawning thread stores the driver here, creates the thread, then the new
/// thread immediately takes ownership. This handoff is protected by a mutex
/// to ensure only one dispatcher is spawned at a time.
static PENDING_DRIVER: Mutex<Option<Box<dyn BlockDriver>>> = Mutex::new(None);

/// Message type for block I/O requests and completions
pub enum BlockMessage {
    ReadRequest {
        sector: u32,
        buffer: *mut [u8],  // Fat pointer (ptr + len)
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
///
/// # Requirements
/// * Buffer length must be a multiple of BLOCK_SIZE (512 bytes)
/// * Buffer must meet DMA alignment requirements (see validate_read_buffer)
pub fn request_read_block(sector: u32, buffer: &mut [u8]) {
    let dispatcher_tid = DISPATCHER_TID.load(Ordering::Relaxed);
    if dispatcher_tid != 0 {
        let requester_tid = thread::Thread::current().id;
        let msg = BlockMessage::ReadRequest {
            sector,
            buffer: buffer as *mut [u8],  // Fat pointer (ptr + len)
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

/// Spawn the block dispatcher thread with the given driver.
///
/// Creates a new thread that owns the driver and handles block I/O requests.
/// The driver is passed to the thread via PENDING_DRIVER since Thread::new()
/// cannot accept closures.
///
/// Returns the thread ID of the newly created dispatcher thread.
pub fn spawn_dispatcher<D: BlockDriver + 'static>(driver: D) -> Result<usize, &'static str> {
    let t = thread::Thread::new(dispatcher_entry);
    let tid = t.id;

    // Store driver for the new thread to pick up
    {
        let mut pending = PENDING_DRIVER.lock();
        if pending.is_some() {
            return Err("Another dispatcher is already being spawned");
        }
        *pending = Some(Box::new(driver));
    }

    thread::add(t);
    Ok(tid)
}

/// Entry point for dispatcher thread.
///
/// This function must have signature `fn()` to work with Thread::new().
/// It retrieves the driver from PENDING_DRIVER and starts the dispatcher
/// main loop.
fn dispatcher_entry() {
    let driver = {
        let mut pending = PENDING_DRIVER.lock();
        pending.take().expect("No driver found for dispatcher")
    };

    dispatcher_main(driver);
}

/// Pending request state
struct PendingRequest {
    requester_tid: usize,
    start_time: u64,
}

static mut PENDING_REQUEST: Option<PendingRequest> = None;

/// Dispatcher thread main loop
fn dispatcher_main(mut device: Box<dyn BlockDriver>) {
    let tid = thread::Thread::current().id;
    let name = device.name();
    kprintln!("Block dispatcher [{}] started (tid={})", name, tid);

    // Register our TID so interrupt handlers can send us messages
    DISPATCHER_TID.store(tid, Ordering::Relaxed);

    kprintln!("Dispatcher [{}]: Ready, waiting for read requests...", name);

    // Main dispatcher loop - handles both VirtIO and SD
    loop {
        let msg = thread::receive_message();
        let block_msg = unsafe { *Box::from_raw(msg.data as *mut BlockMessage) };

        match block_msg {
            BlockMessage::ReadRequest { sector, buffer, requester_tid } => {
                kprintln!("Dispatcher [{}]: ReadRequest from tid={}, sector={}",
                         name, requester_tid, sector);

                // Get caller's buffer
                let buf = unsafe { &mut *buffer };

                // Issue read to device
                let start_time = read_time();
                match device.start_read(sector, buf) {
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
                        kprintln!("Dispatcher [{}]: Failed to issue read: {:?}", name, e);
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
                            kprintln!("Dispatcher [{}]: Read completed in {} us", name, elapsed_us);
                        }
                        Err(e) => {
                            kprintln!("Dispatcher [{}]: Read failed: {:?}", name, e);
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
                    kprintln!("Dispatcher [{}]: ReadComplete but no pending request!", name);
                }
            }
            BlockMessage::ReadResponse { .. } => {
                // Dispatcher should never receive ReadResponse (it only sends them)
                kprintln!("Dispatcher [{}]: ERROR - Received unexpected ReadResponse message!", name);
            }
        }
    }
}
