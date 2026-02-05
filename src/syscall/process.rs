use crate::thread;

pub fn set_tid_address(tf: &mut types::ProcessTrapFrame) {
    // Return the kernel thread ID
    tf.registers.a0 = thread::Thread::current().id;
}

pub fn gettid(tf: &mut types::ProcessTrapFrame) {
    // Return the kernel thread ID
    tf.registers.a0 = thread::Thread::current().id;
}
