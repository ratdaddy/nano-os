//! User process support for kernel threads.
//!
//! Allows user processes to run as kernel threads, enabling preemptive
//! scheduling of user-mode code.

use core::mem;
use core::ptr::write_bytes;
use core::sync::atomic::Ordering;

use crate::kernel_main::KERNEL_READY;
use crate::process;
use crate::process_memory_map;
use crate::process_trampoline;
use crate::thread::{self, Thread};
use crate::vfs;

/// Spawn a user process as a kernel thread.
///
/// Loads the ELF from the given path, initializes the process
/// context, and adds the thread to the scheduler.
///
/// Returns the thread ID of the spawned process thread.
pub fn spawn_process(path: &str) -> Result<usize, &'static str> {
    let mut file = vfs::vfs_open(path).map_err(|_| "failed to open file")?;

    // Create thread first so we can place ProcessTrapFrame on its stack
    let mut thread = Thread::new(user_thread_entry);

    // Place ProcessTrapFrame at top of Thread::stack
    // The Vec's buffer is heap-allocated and stable, so this pointer remains valid
    let stack_top = thread.stack.as_ptr() as usize + thread.stack.len();
    let trap_frame_ptr = (stack_top - mem::size_of::<types::ProcessTrapFrame>())
        as *mut types::ProcessTrapFrame;

    // Adjust thread's sp to be BELOW the trap frame (stack grows downward)
    // This prevents function calls in user_thread_entry from overwriting the trap frame
    thread.context.sp = trap_frame_ptr as usize;

    // Zero-initialize the trap frame and create process context
    let mut process_ctx = unsafe {
        write_bytes(trap_frame_ptr, 0, 1);
        process::Context::new(&mut *trap_frame_ptr)
    };

    // Initialize from ELF (writes entry point, stack, etc. to trap_frame)
    process_memory_map::init_from_elf(&mut file, &mut process_ctx);

    // Set up file descriptors: stdin, stdout, stderr all open to /dev/console
    process_ctx.files.push(Some(vfs::vfs_open("/dev/console").expect("no console")));  // fd 0 (stdin)
    process_ctx.files.push(Some(vfs::vfs_open("/dev/console").expect("no console")));  // fd 1 (stdout)
    process_ctx.files.push(Some(vfs::vfs_open("/dev/console").expect("no console")));  // fd 2 (stderr)

    thread.process = Some(process_ctx);

    // Add to scheduler
    let thread_id = thread::add(thread);

    Ok(thread_id)
}

/// Entry point for kernel threads that run user processes.
///
/// Waits for the block subsystem to finish initializing (signalled by the
/// whole-disk device sda being registered), then enters user mode.
/// TODO: replace with a proper kernel-ready notification.
///
/// Never returns - enters the user-mode trap loop via enter_process.
fn user_thread_entry() {
    while !KERNEL_READY.load(Ordering::Acquire) {
        unsafe { thread::yield_now() };
    }

    let thread = Thread::current();

    let process_ctx = thread
        .process
        .as_mut()
        .expect("user_thread_entry called on thread without process context");

    unsafe {
        process_trampoline::enter_process(process_ctx);
    }
}
