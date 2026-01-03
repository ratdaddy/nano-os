use crate::uart;
use crate::plic;

pub fn write(tf: &mut types::ProcessTrapFrame) {
    println!(
        "[write] fd: {}, buf: {:#x}, count: {}",
        tf.registers.a0,
        tf.registers.a1,
        tf.registers.a2
    );

    let uart = uart::Uart::new(uart::QEMU_UART);
    // test to use NANO_UART
    //let uart = uart::Uart::new(uart::NANO_UART);
    uart.enable_tx_interrupt();
    unsafe {
        plic::init();
    }
    //uart.print_iir();
    uart.write_byte('*' as u8);
    //uart.print_iir();

    // Pretend all bytes were written successfully.
    tf.registers.a0 = tf.registers.a2;
}
