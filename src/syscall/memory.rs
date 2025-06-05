use crate::trap::TrapFrame;

pub fn mmap(tf: &mut TrapFrame) {
    tf.registers.a0 = usize::MAX - 37 + 1;
}

pub fn brk(tf: &mut TrapFrame) {
    println!("brk syscall with addr {:#x}", tf.registers.a0);
    tf.registers.a0 = -12i64 as usize;
}
