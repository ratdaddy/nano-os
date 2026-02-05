//! UART writer thread - handles asynchronous, interrupt-driven UART TX.
//!
//! Other threads send write requests via messages. The writer thread pushes data
//! into a ring buffer, and TX interrupts drain the buffer to the UART FIFO.
//!
//! # Architecture
//!
//! ```text
//! [caller] --WriteData msg--> [uart_writer thread] --push--> [ring buffer]
//!                                                                  |
//!                                                                  v
//!                             [TX interrupt handler] <--pop-- [ring buffer]
//!                                                                  |
//!                                                                  v
//!                                                            [UART FIFO]
//! ```

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::collections::SpscRing;
use crate::drivers::uart;
use crate::file::{self, File, FileOps};
use crate::thread;

// =============================================================================
// Configuration
// =============================================================================

const TX_RING_SIZE: usize = 1024;

// =============================================================================
// Shared State
// =============================================================================

/// The TX ring buffer (SPSC: writer thread produces, interrupt handler consumes).
static TX_RING: SpscRing<TX_RING_SIZE> = SpscRing::new();

/// Thread ID of the uart_writer thread (for message delivery and wakeups).
static WRITER_THREAD_ID: AtomicUsize = AtomicUsize::new(0);

/// Is the TX pump running (interrupt enabled, handler actively draining)?
static TX_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Is the writer thread blocked waiting for buffer space?
static WAITING_FOR_SPACE: AtomicBool = AtomicBool::new(false);

/// Has the interrupt signaled that buffer space is available?
/// Set by notify_tx_ready(), cleared by push_slice_blocking().
static BUFFER_SPACE_AVAILABLE: AtomicBool = AtomicBool::new(false);

// =============================================================================
// Public API
// =============================================================================

/// Initialize the UART writer thread.
pub fn init() {
    let t = thread::Thread::new(writer_entry);
    WRITER_THREAD_ID.store(t.id, Ordering::Relaxed);
    thread::add(t);
}

/// Called from TX interrupt handler to drain the ring buffer to UART.
///
/// This is the "consumer" side of the SPSC ring buffer. If the writer thread
/// is blocked waiting for buffer space, it signals and wakes it.
pub fn notify_tx_ready() {
    drain_ring_to_fifo();

    // If writer thread is blocked waiting for space, signal and wake it
    if WAITING_FOR_SPACE.swap(false, Ordering::AcqRel) {
        BUFFER_SPACE_AVAILABLE.store(true, Ordering::Release);
        let target = WRITER_THREAD_ID.load(Ordering::Relaxed);
        thread::wake_thread(target);
    }
}

/// FileOps implementation for writing to the UART via the writer thread.
pub struct UartFileOps;

impl FileOps for UartFileOps {
    fn write(&self, _file: &mut File, buf: &[u8]) -> Result<usize, file::Error> {
        let len = buf.len();
        send_write(buf);
        Ok(len)
    }
}

/// Static instance of UartFileOps for use with File.
static UART_FILE_OPS: UartFileOps = UartFileOps;

/// Open the UART for writing.
pub fn uart_open() -> File {
    File::new(&UART_FILE_OPS)
}

// =============================================================================
// Writer Thread
// =============================================================================

/// Message type for the writer thread's inbox.
enum WriterMessage {
    WriteData(Vec<u8>),
}

/// Send a write request to the writer thread.
fn send_write(buf: &[u8]) {
    let target = WRITER_THREAD_ID.load(Ordering::Relaxed);
    let sender = thread::Thread::current().id;
    let msg = WriterMessage::WriteData(buf.to_vec());
    let ptr = Box::into_raw(Box::new(msg));
    unsafe {
        thread::send_message_urgent(target, sender, ptr as usize);
    }
}

/// Writer thread entry point.
fn writer_entry() {
    loop {
        let msg = thread::receive_message();
        let writer_msg = unsafe { *Box::from_raw(msg.data as *mut WriterMessage) };

        match writer_msg {
            WriterMessage::WriteData(data) => {
                write_data(&data);
            }
        }
    }
}

/// Write data to the ring buffer with LF→CRLF conversion, then kick-start TX.
fn write_data(data: &[u8]) {
    let mut remaining = data;

    while !remaining.is_empty() {
        // Find next newline (or end of data)
        let newline_pos = remaining.iter().position(|&b| b == b'\n');
        let chunk_end = newline_pos.unwrap_or(remaining.len());

        // Push chunk before newline
        if chunk_end > 0 {
            push_slice_blocking(&remaining[..chunk_end]);
        }

        // Handle newline: convert LF to CRLF
        if newline_pos.is_some() {
            push_slice_blocking(b"\r\n");
            remaining = &remaining[chunk_end + 1..];
        } else {
            remaining = &[];
        }
    }

    // Kick-start TX by filling the FIFO
    drain_ring_to_fifo();
}

/// Push a slice to the ring buffer, blocking if necessary until it fits.
fn push_slice_blocking(data: &[u8]) {
    let mut remaining = data;

    while !remaining.is_empty() {
        let pushed = TX_RING.push_slice(remaining);

        if pushed == 0 {
            // Buffer full - wait for interrupt to drain some data
            TX_ACTIVE.store(true, Ordering::Release);
            uart::get().enable_tx_interrupt();
            BUFFER_SPACE_AVAILABLE.store(false, Ordering::Release);
            WAITING_FOR_SPACE.store(true, Ordering::Release);

            // Loop until interrupt signals space is available
            // (may be woken spuriously by incoming WriteData messages)
            while !BUFFER_SPACE_AVAILABLE.load(Ordering::Acquire) {
                unsafe { thread::block_now(); }
            }

            WAITING_FOR_SPACE.store(false, Ordering::Release);
        } else {
            remaining = &remaining[pushed..];
        }
    }
}

// =============================================================================
// TX Pump (Ring Buffer → UART FIFO)
// =============================================================================

/// Drain the ring buffer into the UART FIFO.
///
/// Waits for FIFO to be ready, then writes up to TX_FIFO_SIZE bytes.
/// Manages TX_ACTIVE flag and interrupt enable/disable state.
fn drain_ring_to_fifo() {
    let uart = uart::get();

    // Wait for FIFO to be ready (THRE=1 means holding register empty).
    // This should rarely spin - only if lower-level diagnostic code
    // is using synchronous writes that haven't finished yet.
    while !uart.tx_ready() {}

    // Fill up to TX_FIFO_SIZE bytes without re-checking THRE
    // (THRE goes LOW after first write even though FIFO has space)
    for _ in 0..uart::TX_FIFO_SIZE {
        if let Some(byte) = TX_RING.pop() {
            uart.write_byte_nowait(byte);
        } else {
            // Ring buffer empty - disable TX interrupt
            TX_ACTIVE.store(false, Ordering::Release);
            uart.disable_tx_interrupt();
            return;
        }
    }

    // Filled FIFO but ring buffer still has data - ensure interrupt stays enabled
    if !TX_RING.is_empty() {
        TX_ACTIVE.store(true, Ordering::Release);
        uart.enable_tx_interrupt();
    }
}
