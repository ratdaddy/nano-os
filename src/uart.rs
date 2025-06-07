#[derive(Copy, Clone)]
pub struct UartConfig {
    pub base: usize,
    pub reg_shift: usize,     // log2(word spacing): 0 for ns16550a, 2 for dw-apb-uart
    pub reg_io_width: usize,  // in bytes: 1 or 4 (use 4 for dw-apb-uart)
}

pub struct Uart {
    config: UartConfig,
}

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
