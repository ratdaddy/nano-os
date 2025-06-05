use crate::trap::TrapFrame;

/// Handle the `mmap` syscall.
pub fn mmap(tf: &mut TrapFrame) {
    // For now just return an arbitrary unmapped address as before.
    tf.registers.a0 = usize::MAX - 37 + 1;
}

/// Handle the `brk` syscall.
pub fn brk(tf: &mut TrapFrame) {
    println!("brk syscall with addr {:#x}", tf.registers.a0);
    // Indicate failure just like the previous implementation.
    tf.registers.a0 = -12i64 as usize;
}
