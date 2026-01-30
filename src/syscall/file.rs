use crate::drivers::uart;

pub fn write(tf: &mut types::ProcessTrapFrame) {
    println!(
        "[write] fd: {}, buf: {:#x}, count: {}",
        tf.registers.a0,
        tf.registers.a1,
        tf.registers.a2
    );

    uart::get().enable_tx_interrupt();
    print!("%");

    // Pretend all bytes were written successfully.
    tf.registers.a0 = tf.registers.a2;
}
