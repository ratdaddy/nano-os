use crate::trap::TrapFrame;

/// Handle the `write` syscall.
pub fn write(tf: &mut TrapFrame) {
    println!(
        "write syscall with fd {}, buf {:#x}, count {}",
        tf.registers.a0,
        tf.registers.a1,
        tf.registers.a2
    );
    // Pretend all bytes were written successfully.
    tf.registers.a0 = tf.registers.a2;
}
