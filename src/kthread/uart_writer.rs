use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::file_ops::{self, FileOps};
use crate::thread;
use crate::drivers::uart;

static WRITER_THREAD_ID: AtomicUsize = AtomicUsize::new(0);

/// Send a write buffer to the UART writer thread.
/// Takes ownership of the data via Box::into_raw; the writer thread
/// reconstructs and frees it after writing.
/// Uses send_message_urgent so the writer thread runs immediately.
fn send_write(buf: &[u8]) {
    let data = buf.to_vec();
    let ptr = Box::into_raw(Box::new(data));
    unsafe {
        thread::send_message_urgent(
            WRITER_THREAD_ID.load(Ordering::Relaxed),
            thread::Thread::current().id,
            ptr as usize,
        );
    }
}

pub struct UartFileOps;

impl FileOps for UartFileOps {
    fn write(&mut self, buf: &[u8]) -> Result<usize, file_ops::Error> {
        let len = buf.len();
        send_write(buf);
        Ok(len)
    }
}

/// Buffered writer for kprint!/kprintln! macros.
/// Accumulates formatted output, then sends it as a single message on drop.
pub struct KPrintWriter {
    buf: String,
}

impl KPrintWriter {
    pub fn new() -> Self {
        Self { buf: String::new() }
    }
}

impl core::fmt::Write for KPrintWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.buf.push_str(s);
        Ok(())
    }
}

impl Drop for KPrintWriter {
    fn drop(&mut self) {
        if !self.buf.is_empty() {
            let mut uart = UartFileOps;
            let _ = uart.write(self.buf.as_bytes());
        }
    }
}

#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let mut writer = $crate::kthread::uart_writer::KPrintWriter::new();
        let _ = write!(writer, $($arg)*);
    }};
}

#[macro_export]
macro_rules! kprintln {
    () => { $crate::kprint!("\n") };
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let mut writer = $crate::kthread::uart_writer::KPrintWriter::new();
        let _ = writeln!(writer, $($arg)*);
    }};
}

pub fn init() {
    let t = thread::Thread::new(writer_entry);
    WRITER_THREAD_ID.store(t.id, Ordering::Relaxed);
    thread::add(t);
}

fn writer_entry() {
    loop {
        let msg = thread::receive_message();
        let data = unsafe { Box::from_raw(msg.data as *mut Vec<u8>) };
        for &byte in data.iter() {
            if byte == b'\n' {
                uart::get().write_byte(b'\r');
            }
            uart::get().write_byte(byte);
        }
    }
}
