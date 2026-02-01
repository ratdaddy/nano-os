use crate::thread;
use crate::kprintln;

/// Run the UART TX flood test in a thread context
pub fn run() -> ! {
    println!("=== UART TX Flood Test ===");

    let flood_thread = thread::Thread::new(flood_entry);
    thread::add(flood_thread);

    thread::start_scheduler()
}

fn flood_entry() {
    println!("Starting UART flood test");

    // Write 100 lines of 40 chars each = 4000+ chars (with newlines)
    for i in 0..100 {
        kprintln!("{:03}: 0123456789012345678901234567890123456789", i);
    }

    println!("Flood test complete");
    thread::exit();
}
