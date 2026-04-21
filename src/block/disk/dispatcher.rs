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
//! via `DRIVER`: the spawning code stores the driver there, creates the
//! thread, and the new thread immediately takes ownership.

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

use crate::drivers::{BlockDriver, BlockError};
#[cfg(feature = "trace_volumes")]
use crate::dtb;
use crate::thread;

/// Read the RISC-V time register
#[cfg(feature = "trace_volumes")]
#[inline]
fn read_time() -> u64 {
    let time: u64;
    unsafe {
        core::arch::asm!("rdtime {}", out(reg) time);
    }
    time
}

static DISPATCHER_TID: AtomicUsize = AtomicUsize::new(0);

/// Temporary slot for passing the driver to the dispatcher thread on startup.
///
/// The spawning thread stores the driver here, creates the thread, then the new
/// thread immediately takes ownership. Protected by a mutex to ensure only one
/// dispatcher is spawned at a time.
static DRIVER: Mutex<Option<Box<dyn BlockDriver>>> = Mutex::new(None);

/// Message type for block I/O requests and completions
#[derive(Debug)]
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

// SAFETY: BlockMessage::ReadRequest carries a raw pointer to the caller's buffer.
// This is safe to send across threads because the requester blocks waiting for a
// ReadResponse after sending, so the buffer is only accessed by the dispatcher
// while the requester is suspended — no concurrent access is possible.
unsafe impl Send for BlockMessage {}

/// A read request waiting to be issued once the current in-flight read completes.
struct PendingRequest {
    sector: u32,
    buffer: *mut [u8],
    requester_tid: usize,
}

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
/// The driver is passed to the thread via DRIVER since Thread::new()
/// cannot accept closures.
///
/// Returns the thread ID of the newly created dispatcher thread.
pub(super) fn spawn_dispatcher<D: BlockDriver + 'static>(driver: D) -> Result<usize, &'static str> {
    let t = thread::Thread::new(dispatcher_entry);
    let tid = t.id;

    driver.set_completion_handler(send_read_completion);

    // Store driver for the new thread to pick up. The block ensures the guard
    // is dropped before thread::add() — if the scheduler immediately runs the
    // new thread, dispatcher_entry() will call DRIVER.lock() and deadlock if
    // we still hold it.
    {
        let mut guard = DRIVER.lock();
        if guard.is_some() {
            return Err("Another dispatcher is already being spawned");
        }
        *guard = Some(Box::new(driver));
    }

    // Register TID before adding the thread so that request_read_block()
    // can send messages as soon as block_init starts issuing reads, even
    // before the dispatcher thread has had a chance to run.
    DISPATCHER_TID.store(tid, Ordering::Relaxed);

    thread::add(t);
    Ok(tid)
}

/// Entry point for dispatcher thread.
///
/// This function must have signature `fn()` to work with Thread::new().
/// It retrieves the driver from DRIVER and starts the dispatcher
/// main loop.
fn dispatcher_entry() {
    let driver = {
        let mut guard = DRIVER.lock();
        guard.take().expect("No driver found for dispatcher")
    };

    dispatcher_main(driver);
}

/// State for a request issued to hardware, awaiting a completion interrupt.
#[derive(Debug)]
struct Request {
    requester_tid: usize,
    #[cfg(feature = "trace_volumes")]
    start_time: u64,
}

static mut REQUEST: Option<Request> = None;

/// Issue a read to the device and record it as the in-flight request.
/// On device error, sends a ReadResponse directly to the requester instead.
fn issue_read(
    device: &mut dyn BlockDriver,
    name: &str,
    dispatcher_tid: usize,
    sector: u32,
    buffer: *mut [u8],
    requester_tid: usize,
) {
    let buf = unsafe { &mut *buffer };

    #[cfg(feature = "trace_volumes")]
    kprintln!("dispatcher [{}]: issuing read sector={}", name, sector);

    #[cfg(feature = "trace_volumes")]
    let start_time = read_time();

    match device.start_read(sector, buf) {
        Ok(_) => {
            unsafe {
                let ptr = &raw mut REQUEST;
                *ptr = Some(Request {
                    requester_tid,
                    #[cfg(feature = "trace_volumes")]
                    start_time,
                });
            }
        }
        Err(e) => {
            kprintln!("dispatcher [{}]: failed to issue read: {:?}", name, e);
            let response = BlockMessage::ReadResponse { status: Err(e) };
            let ptr = Box::into_raw(Box::new(response));
            thread::send_message(requester_tid, thread::Message {
                sender: dispatcher_tid,
                data: ptr as usize,
            });
        }
    }
}

/// Dispatcher thread main loop
fn dispatcher_main(mut device: Box<dyn BlockDriver>) {
    let tid = thread::Thread::current().id;
    let name = device.name();
    kprintln!("Block dispatcher [{}] started (tid={})", name, tid);

    let mut pending: VecDeque<PendingRequest> = VecDeque::new();

    loop {
        let msg = thread::receive_message();
        let block_msg = unsafe { *Box::from_raw(msg.data as *mut BlockMessage) };

        match block_msg {
            BlockMessage::ReadRequest { sector, buffer, requester_tid } => {
                let in_flight = unsafe { (*(&raw const REQUEST)).is_some() };
                if in_flight {
                    #[cfg(feature = "trace_volumes")]
                    kprintln!("dispatcher [{}]: queuing read sector={} (read in flight)", name, sector);
                    pending.push_back(PendingRequest { sector, buffer, requester_tid });
                } else {
                    issue_read(device.as_mut(), name, tid, sector, buffer, requester_tid);
                }
            }
            BlockMessage::ReadComplete { status } => {
                let request = unsafe {
                    let ptr = &raw mut REQUEST;
                    (*ptr).take()
                };

                if let Some(req) = request {
                    #[cfg(feature = "trace_volumes")]
                    if status.is_ok() {
                        let end_time = read_time();
                        let elapsed_cycles = end_time - req.start_time;
                        let timebase_freq = dtb::get_timebase_frequency();
                        let elapsed_us = (elapsed_cycles * 1_000_000) / timebase_freq;
                        kprintln!("dispatcher [{}]: read completed in {} us", name, elapsed_us);
                    }

                    match status {
                        Ok(()) => {}
                        Err(e) => {
                            kprintln!("dispatcher [{}]: read failed: {:?}", name, e);
                        }
                    }

                    // Send response to requester
                    let response = BlockMessage::ReadResponse { status };
                    let ptr = Box::into_raw(Box::new(response));
                    thread::send_message(req.requester_tid, thread::Message {
                        sender: tid,
                        data: ptr as usize,
                    });

                    // Issue next queued read if any
                    if let Some(next) = pending.pop_front() {
                        #[cfg(feature = "trace_volumes")]
                        kprintln!("dispatcher [{}]: dequeuing next read sector={}", name, next.sector);
                        issue_read(device.as_mut(), name, tid, next.sector, next.buffer, next.requester_tid);
                    }
                } else {
                    kprintln!("dispatcher [{}]: ReadComplete but no pending request!", name);
                }
            }
            BlockMessage::ReadResponse { .. } => {
                // Dispatcher should never receive ReadResponse (it only sends them)
                kprintln!("dispatcher [{}]: ERROR - received unexpected ReadResponse", name);
            }
        }
    }
}
