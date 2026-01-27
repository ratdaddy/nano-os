use core::sync::atomic::{AtomicUsize, Ordering};
use crate::thread;

static THREAD_A_ID: AtomicUsize = AtomicUsize::new(0);
static THREAD_B_ID: AtomicUsize = AtomicUsize::new(0);

/// Thread A: sends a message to B, then waits for B's reply
fn thread_a_entry() {
    let my_id = thread::Thread::current().id;
    let peer_id = THREAD_B_ID.load(Ordering::Relaxed);

    kprintln!("[Thread A ({})] Starting", my_id);

    kprintln!("[Thread A ({})] Sending message (data=42) to Thread B ({})", my_id, peer_id);
    thread::send_message(peer_id, thread::Message { sender: my_id, data: 42 });
    kprintln!("[Thread A ({})] Message sent, waiting for reply...", my_id);

    let reply = thread::receive_message();
    kprintln!("[Thread A ({})] Received reply from Thread {} with data: {}", my_id, reply.sender, reply.data);

    kprintln!("[Thread A ({})] Done, exiting", my_id);
    thread::exit();
}

/// Thread B: waits for a message from A, then sends a reply back
fn thread_b_entry() {
    let my_id = thread::Thread::current().id;
    let peer_id = THREAD_A_ID.load(Ordering::Relaxed);

    kprintln!("[Thread B ({})] Starting", my_id);

    kprintln!("[Thread B ({})] Waiting for message...", my_id);
    let msg = thread::receive_message();
    kprintln!("[Thread B ({})] Received message from Thread {} with data: {}", my_id, msg.sender, msg.data);

    kprintln!("[Thread B ({})] Sending reply (data=99) to Thread A ({})", my_id, peer_id);
    thread::send_message(peer_id, thread::Message { sender: my_id, data: 99 });
    kprintln!("[Thread B ({})] Reply sent, exiting", my_id);

    thread::exit();
}

/// Message passing demo - creates two threads that exchange messages
/// Note: This function never returns (start_scheduler is divergent)
pub fn test_message_passing() -> ! {
    println!("=== Message Passing Demo ===");

    let thread_a = thread::Thread::new(thread_a_entry);
    let a_id = thread_a.id;
    THREAD_A_ID.store(a_id, Ordering::Relaxed);
    println!("Thread A ({}) created: sp={:#x}, ra={:#x}",
             a_id, thread_a.context.sp, thread_a.context.ra);
    thread::add(thread_a);

    let thread_b = thread::Thread::new(thread_b_entry);
    let b_id = thread_b.id;
    THREAD_B_ID.store(b_id, Ordering::Relaxed);
    println!("Thread B ({}) created: sp={:#x}, ra={:#x}",
             b_id, thread_b.context.sp, thread_b.context.ra);
    thread::add(thread_b);

    println!("Starting scheduler...");
    thread::start_scheduler()
}
