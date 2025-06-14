/// Temporary handler for a group of stubbed signal related syscalls.
/// Returns success without doing any work.
pub fn generic_stub(tf: &mut types::ProcessTrapFrame) {
    tf.registers.a0 = 0;
}
