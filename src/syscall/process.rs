use crate::trap::TrapFrame;

pub fn set_tid_address(tf: &mut TrapFrame) {
    // The kernel returns the thread id.  Until threads are implemented
    // just return a constant non-zero value.
    tf.registers.a0 = 1;
}
