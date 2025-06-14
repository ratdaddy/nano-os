pub fn write(tf: &mut types::ProcessTrapFrame) {
    println!(
        "[write] fd: {}, buf: {:#x}, count: {}",
        tf.registers.a0,
        tf.registers.a1,
        tf.registers.a2
    );
    // Pretend all bytes were written successfully.
    tf.registers.a0 = tf.registers.a2;
}
