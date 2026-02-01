//! Kernel print macros and supporting infrastructure.
//!
//! Provides `kprint!` and `kprintln!` macros for kernel-space output that goes
//! through the UART writer thread (non-blocking, interrupt-driven).

use alloc::string::String;
use crate::file_ops::FileOps;
use crate::kthread::uart_writer::UartFileOps;

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
            let mut uart = UartFileOps;
            let _ = uart.write(self.buf.as_bytes());
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
