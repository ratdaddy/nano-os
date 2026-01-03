const PLIC_BASE: usize = 0x0c00_0000; // QEMU PLIC base address
//const PLIC_BASE: usize = 0x7000_0000; // NanoRV PLIC base address
const PLIC_PRIORITY: usize = PLIC_BASE + 0x0000;
const PLIC_ENABLE: usize = PLIC_BASE + 0x2000;
const PLIC_CONTEXT: usize = PLIC_BASE + 0x200000 + 0x1000 * 1; // For S-mode on hart 0

const UART_IRQ_ID: u32 = 10; // QEMU UART IRQ ID
//const UART_IRQ_ID: u32 = 0x2c; // NanoRV UART IRQ ID

pub unsafe fn init() {
    // Set UART interrupt priority to 1
    ((PLIC_PRIORITY + (UART_IRQ_ID as usize) * 4) as *mut u32).write_volatile(1);

    // Enable UART interrupt for S-mode hart 0
    let enable_base = PLIC_ENABLE + 0x80 * 1; // hart 0, S-mode context 1
    let irq_bit = 1 << (UART_IRQ_ID % 32);
    (enable_base as *mut u32).write_volatile(irq_bit);

    // Set priority threshold to 0 to allow all
    ((PLIC_CONTEXT + 0x000) as *mut u32).write_volatile(0);
}
