use core::sync::atomic::{AtomicUsize, Ordering};
use crate::chardev;
use crate::dtb;
use crate::file::{self, File, FileOps};
use crate::kthread::uart_writer;

#[derive(Copy, Clone)]
struct UartConfig {
    base: usize,
    reg_shift: usize,     // log2(word spacing): 0 for ns16550a, 2 for dw-apb-uart
    reg_io_width: usize,  // in bytes: 1 or 4 (use 4 for dw-apb-uart)
}

// For QEMU `virt` machine with ns16550a UART
const QEMU_UART: UartConfig = UartConfig {
    base: 0x1000_0000,
    reg_shift: 0,
    reg_io_width: 1,
};

// For NanoKVM's dw-apb-uart at serial@04140000
const NANO_UART: UartConfig = UartConfig {
    base: 0x0414_0000,
    reg_shift: 2,
    reg_io_width: 4,
};

// UART IRQ numbers from DTB (same device node as reg base)
pub const QEMU_UART_IRQ: u32 = 10;
pub const NANO_UART_IRQ: u32 = 0x2c;  // 44 decimal

/// 16550 UART TX FIFO size in bytes.
pub const TX_FIFO_SIZE: usize = 16;

// Store config in atomics to avoid mutable static references
static UART_BASE: AtomicUsize = AtomicUsize::new(0);
static UART_REG_SHIFT: AtomicUsize = AtomicUsize::new(0);
static UART_REG_IO_WIDTH: AtomicUsize = AtomicUsize::new(0);

/// FileOps implementation for writing to the UART via the writer thread.
struct UartFileOps;

impl FileOps for UartFileOps {
    fn write(&self, _file: &mut File, buf: &[u8]) -> Result<usize, file::Error> {
        let len = buf.len();
        uart_writer::send_write(buf);
        Ok(len)
    }
}

static UART_FILE_OPS: UartFileOps = UartFileOps;

/// Register the UART as a character device (major 5, minor 1).
/// Must be called after VFS is initialized.
pub fn register_chrdev() {
    chardev::chrdev_register(5, 1, &UART_FILE_OPS);
}

/// Initialize the UART driver. Must be called after dtb::init() and plic::init().
pub fn init() {
    let (config, irq) = match dtb::get_cpu_type() {
        dtb::CpuType::LicheeRVNano => (NANO_UART, NANO_UART_IRQ),
        _ => (QEMU_UART, QEMU_UART_IRQ),
    };
    UART_BASE.store(config.base, Ordering::Relaxed);
    UART_REG_SHIFT.store(config.reg_shift, Ordering::Relaxed);
    UART_REG_IO_WIDTH.store(config.reg_io_width, Ordering::Relaxed);

    // Enable FIFOs (required for 16550 to use 16-byte TX/RX buffers)
    let uart = Uart {
        base: config.base,
        reg_shift: config.reg_shift,
        reg_io_width: config.reg_io_width,
    };
    const FCR_OFFSET: usize = 2;
    const FCR_FIFO_ENABLE: u8 = 0x01;
    const FCR_RX_RESET: u8 = 0x02;
    const FCR_TX_RESET: u8 = 0x04;
    unsafe {
        uart.write_reg(FCR_OFFSET, FCR_FIFO_ENABLE | FCR_RX_RESET | FCR_TX_RESET);
    }

    // Register UART IRQ handler with PLIC
    crate::drivers::plic::register_irq(irq, handle_irq);
}

/// Get a UART handle for performing operations.
/// Panics if init() hasn't been called.
pub fn get() -> Uart {
    let base = UART_BASE.load(Ordering::Relaxed);
    assert!(base != 0, "UART not initialized - call uart::init() first");
    Uart {
        base,
        reg_shift: UART_REG_SHIFT.load(Ordering::Relaxed),
        reg_io_width: UART_REG_IO_WIDTH.load(Ordering::Relaxed),
    }
}

#[derive(Copy, Clone)]
pub struct Uart {
    base: usize,
    reg_shift: usize,
    reg_io_width: usize,
}

impl Uart {
    // Compute address of register `n` with shift
    fn reg_addr(&self, n: usize) -> *mut u8 {
        (self.base + (n << self.reg_shift)) as *mut u8
    }

    fn reg_addr_32(&self, n: usize) -> *mut u32 {
        (self.base + (n << self.reg_shift)) as *mut u32
    }

    /// Polling-based transmit
    pub fn write_byte(&self, byte: u8) {
        const LSR_OFFSET: usize = 5;
        const THR_OFFSET: usize = 0;
        const LSR_THRE: u8 = 1 << 5;

        unsafe {
            // Wait until Transmit Holding Register Empty
            while self.read_reg(LSR_OFFSET) & LSR_THRE == 0 {}

            self.write_reg(THR_OFFSET, byte);
        }
    }

    pub fn write_str(&self, s: &str) {
        for byte in s.bytes() {
            self.write_byte(byte);
        }
    }

    pub fn enable_tx_interrupt(&self) {
        const IER_OFFSET: usize = 1;
        const IER_THRE: u8 = 1 << 1;

        unsafe {
            let current = self.read_reg(IER_OFFSET);
            self.write_reg(IER_OFFSET, current | IER_THRE);
        }
    }

    pub fn disable_tx_interrupt(&self) {
        const IER_OFFSET: usize = 1;
        const IER_THRE: u8 = 1 << 1;

        unsafe {
            let current = self.read_reg(IER_OFFSET);
            self.write_reg(IER_OFFSET, current & !IER_THRE);
        }
    }

    /// Write a byte without waiting for THR to be empty.
    /// Caller must ensure THR is ready (e.g., after receiving TxReady interrupt).
    pub fn write_byte_nowait(&self, byte: u8) {
        const THR_OFFSET: usize = 0;
        unsafe {
            self.write_reg(THR_OFFSET, byte);
        }
    }

    /// Check if transmit holding register is empty (ready for next byte)
    pub fn tx_ready(&self) -> bool {
        const LSR_OFFSET: usize = 5;
        const LSR_THRE: u8 = 1 << 5;
        unsafe { self.read_reg(LSR_OFFSET) & LSR_THRE != 0 }
    }

    pub fn enable_rx_interrupt(&self) {
        const IER_OFFSET: usize = 1;
        const IER_RDA: u8 = 1 << 0;  // Received Data Available interrupt

        unsafe {
            let current = self.read_reg(IER_OFFSET);
            self.write_reg(IER_OFFSET, current | IER_RDA);
        }
    }

    pub fn read_byte(&self) -> Option<u8> {
        const LSR_OFFSET: usize = 5;
        const RBR_OFFSET: usize = 0;
        const LSR_DR: u8 = 1 << 0;  // Data Ready bit

        unsafe {
            // Check if data is available
            if self.read_reg(LSR_OFFSET) & LSR_DR != 0 {
                Some(self.read_reg(RBR_OFFSET))
            } else {
                None
            }
        }
    }

    unsafe fn read_reg(&self, offset: usize) -> u8 {
        if self.reg_io_width == 4 {
            self.reg_addr_32(offset).read_volatile() as u8
        } else {
            self.reg_addr(offset).read_volatile()
        }
    }

    unsafe fn write_reg(&self, offset: usize, val: u8) {
        if self.reg_io_width == 4 {
            self.reg_addr_32(offset).write_volatile(val as u32);
        } else {
            self.reg_addr(offset).write_volatile(val);
        }
    }
}

/// Handle UART interrupt. Called by PLIC dispatch when UART IRQ fires.
/// Checks IIR to determine interrupt type (RX/TX) and handles accordingly.
pub fn handle_irq(_irq: u32) {
    const IIR_OFFSET: usize = 2;
    const IIR_NO_INTERRUPT: u8 = 0x01;
    const IIR_ID_MASK: u8 = 0x0E;
    const IIR_RX_DATA: u8 = 0x04;      // Received data available
    const IIR_TX_EMPTY: u8 = 0x02;     // Transmitter holding register empty
    const IIR_RX_TIMEOUT: u8 = 0x0C;   // Character timeout

    let uart = get();

    loop {
        let iir = unsafe { uart.read_reg(IIR_OFFSET) };

        if iir & IIR_NO_INTERRUPT != 0 {
            break; // No more pending interrupts
        }

        match iir & IIR_ID_MASK {
            IIR_RX_DATA | IIR_RX_TIMEOUT => {
                // Drain all available bytes from RX FIFO
                while let Some(byte) = uart.read_byte() {
                    println!("UART RX: {:#x} ('{}')", byte, byte as char);
                }
            }
            IIR_TX_EMPTY => {
                // TX buffer empty - notify uart_writer thread, then break.
                // We only handle TX once per interrupt - even if more data
                // becomes ready, we'll get another interrupt.
                uart_writer::notify_tx_ready();
                break;
            }
            id => {
                println!("UART: unhandled interrupt type {:#x}", id);
            }
        }
    }
}
