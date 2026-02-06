//! Kernel print macros and supporting infrastructure.
//!
//! Provides `kprint!` and `kprintln!` macros for kernel-space output that goes
//! through the UART writer thread (non-blocking, interrupt-driven).
//!
//! Call `init()` once after the UART writer thread is initialized to cache
//! the console file handle.

use alloc::string::String;
use crate::file::File;
use crate::kthread::uart_writer;
use crate::vfs;

/// Cached console file handle, opened once during init().
static mut CONSOLE: Option<File> = None;

/// Initialize kprint by opening and caching the console file.
/// Call this after the UART writer thread is initialized.
pub fn init() {
    // For now, use uart_open(). Later: vfs_open("/dev/console")
    unsafe {
        CONSOLE = Some(uart_writer::uart_open());
    }
}

/// Get the cached console file handle.
fn console() -> &'static mut File {
    unsafe {
        (*core::ptr::addr_of_mut!(CONSOLE))
            .as_mut()
            .expect("kprint::init() not called")
    }
}

/// Buffered writer for kprint!/kprintln! macros.
///
/// Collects formatted output into a buffer, then sends it to the UART writer
/// thread when dropped. This batches the write into a single message.
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
            let _ = vfs::vfs_write(console(), self.buf.as_bytes());
        }
    }
}

/// Print formatted text to the kernel console (no newline).
#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let mut writer = $crate::kprint::KPrintWriter::new();
        let _ = write!(writer, $($arg)*);
    }};
}

/// Print formatted text to the kernel console with a newline.
#[macro_export]
macro_rules! kprintln {
    () => { $crate::kprint!("\n") };
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let mut writer = $crate::kprint::KPrintWriter::new();
        let _ = writeln!(writer, $($arg)*);
    }};
}
