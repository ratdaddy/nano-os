use crate::drivers::uart;
use crate::drivers::plic;
use crate::dtb;

pub fn write(tf: &mut types::ProcessTrapFrame) {
    println!(
        "[write] fd: {}, buf: {:#x}, count: {}",
        tf.registers.a0,
        tf.registers.a1,
        tf.registers.a2
    );

    // Dynamically select UART based on CPU type
    let uart_config = match dtb::get_cpu_type() {
        dtb::CpuType::Qemu => uart::QEMU_UART,
        dtb::CpuType::LicheeRVNano => uart::NANO_UART,
        _ => {
            println!("WARNING: Unknown CPU type, defaulting to QEMU UART");
            uart::QEMU_UART
        }
    };

    println!("Using UART at base {:#x}", uart_config.base);
    let uart = uart::Uart::new(uart_config);
    uart.enable_tx_interrupt();
    unsafe {
        plic::init();
    }
    // uart.write_byte('*' as u8);
    print!("%");

    // Pretend all bytes were written successfully.
    tf.registers.a0 = tf.registers.a2;
}
