use crate::thread;

/// Entry point for test threads - demonstrates yield and exit
fn test_thread_entry() {
    let me = thread::Thread::current();
    let my_id = me.id;

    println!("Thread {} starting", my_id);
    unsafe { thread::yield_now(); }

    println!("Thread {} resumed after yield", my_id);

    thread::exit();
}

/// Test kernel threading - creates threads and starts the scheduler
/// Note: This function never returns (start_scheduler is divergent)
#[allow(dead_code)]
pub fn test_threading() -> ! {
    println!("Creating test threads...");

    let thread1 = thread::Thread::new(test_thread_entry);
    let id1 = thread1.id;
    println!("Thread {} created: sp={:#x}, ra={:#x}",
             id1, thread1.context.sp, thread1.context.ra);
    thread::add(thread1);

    let thread2 = thread::Thread::new(test_thread_entry);
    let id2 = thread2.id;
    println!("Thread {} created: sp={:#x}, ra={:#x}",
             id2, thread2.context.sp, thread2.context.ra);
    thread::add(thread2);

    println!("Starting scheduler...");
    thread::start_scheduler()
}
