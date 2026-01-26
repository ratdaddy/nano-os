#[derive(Copy, Clone)]
pub struct UartConfig {
    pub base: usize,
    pub reg_shift: usize,     // log2(word spacing): 0 for ns16550a, 2 for dw-apb-uart
    pub reg_io_width: usize,  // in bytes: 1 or 4 (use 4 for dw-apb-uart)
}

pub struct Uart {
    config: UartConfig,
}

// For QEMU `virt` machine with ns16550a UART
pub const QEMU_UART: UartConfig = UartConfig {
    base: 0x1000_0000,
    reg_shift: 0,
    reg_io_width: 1,
};

// For NanoKVM's dw-apb-uart at serial@04140000
pub const NANO_UART: UartConfig = UartConfig {
    base: 0x0414_0000,
    reg_shift: 2,
    reg_io_width: 4,
};

impl Uart {
    pub const fn new(config: UartConfig) -> Self {
        Self { config }
    }

    // Compute address of register `n` with shift
    fn reg_addr(&self, n: usize) -> *mut u8 {
        (self.config.base + (n << self.config.reg_shift)) as *mut u8
    }

    fn reg_addr_32(&self, n: usize) -> *mut u32 {
        (self.config.base + (n << self.config.reg_shift)) as *mut u32
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

    pub fn enable_rx_interrupt(&self) {
        const IER_OFFSET: usize = 1;
        const IER_RDA: u8 = 1 << 0;  // Received Data Available interrupt

        unsafe {
            let current = self.read_reg(IER_OFFSET);
            self.write_reg(IER_OFFSET, current | IER_RDA);
        }
    }

    #[allow(dead_code)]
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
        if self.config.reg_io_width == 4 {
            self.reg_addr_32(offset).read_volatile() as u8
        } else {
            self.reg_addr(offset).read_volatile()
        }
    }

    unsafe fn write_reg(&self, offset: usize, val: u8) {
        if self.config.reg_io_width == 4 {
            self.reg_addr_32(offset).write_volatile(val as u32);
        } else {
            self.reg_addr(offset).write_volatile(val);
        }
    }
}
