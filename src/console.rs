use core::fmt::{self, Write};

#[inline(always)]
pub fn sbi_console_putchar(ch: u8) {
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") ch as usize,
            in("a7") 0x01,
            in("a6") 0x00,
            lateout("a0") _,
            options(nostack, nomem),
        );
    }
}

pub struct PutcharWriter;

impl Write for PutcharWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            if c.is_ascii() {
                sbi_console_putchar(c as u8);
            } else {
                sbi_console_putchar(b'?');
            }
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let _ = write!(console::PutcharWriter, $($arg)*);
    }};
}

#[macro_export]
macro_rules! println {
    () => {
        print!("\n");
    };
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let _ = writeln!(console::PutcharWriter, $($arg)*);
    }};
}
