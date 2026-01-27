use crate::dtb;

const QEMU_PLIC_BASE: usize = 0x0c00_0000;
const NANO_PLIC_BASE: usize = 0x7000_0000;

const QEMU_UART_IRQ_ID: u32 = 10;
const NANO_UART_IRQ_ID: u32 = 0x2c;

pub unsafe fn init() {
    // Dynamically select PLIC base and UART IRQ based on CPU type
    let (plic_base, uart_irq_id) = match dtb::get_cpu_type() {
        dtb::CpuType::Qemu => (QEMU_PLIC_BASE, QEMU_UART_IRQ_ID),
        dtb::CpuType::LicheeRVNano => (NANO_PLIC_BASE, NANO_UART_IRQ_ID),
        _ => {
            println!("WARNING: Unknown CPU type, defaulting to QEMU");
            (QEMU_PLIC_BASE, QEMU_UART_IRQ_ID)
        }
    };

    println!("PLIC: using base {:#x}, UART IRQ {}", plic_base, uart_irq_id);

    let plic_priority = plic_base + 0x0000;
    let plic_enable = plic_base + 0x2000;
    let plic_context = plic_base + 0x200000 + 0x1000 * 1; // For S-mode on hart 0

    // Set UART interrupt priority to 1
    ((plic_priority + (uart_irq_id as usize) * 4) as *mut u32).write_volatile(1);

    // Enable UART interrupt for S-mode hart 0
    let enable_base = plic_enable + 0x80 * 1; // hart 0, S-mode context 1
    let word_index = (uart_irq_id / 32) as usize;
    let irq_bit = 1 << (uart_irq_id % 32);
    let enable_word = enable_base + word_index * 4;
    (enable_word as *mut u32).write_volatile(irq_bit);

    // Read back the enable register to verify write
    let readback = (enable_word as *mut u32).read_volatile();
    println!("PLIC enable[{}] write: {:#x}, readback: {:#x}", word_index, irq_bit, readback);

    // Set priority threshold to 0 to allow all
    ((plic_context + 0x000) as *mut u32).write_volatile(0);
}
