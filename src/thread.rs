use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;
use types::ThreadContext;

/// Thread execution state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Running,
    Ready,
    Blocked,
}

/// Message passed between kernel threads
pub struct Message {
    pub sender: usize,
    pub data: usize,
}

const STACK_SIZE: usize = 8 * 1024; // 8 KB stack

/// Kernel thread structure
pub struct Thread {
    pub id: usize,
    pub state: ThreadState,

    // Kernel execution context (saved/restored during context switch)
    pub context: ThreadContext,

    // Stack is allocated separately on heap to avoid stack overflow during construction
    pub stack: Vec<u8>,

    // Message inbox for inter-thread communication
    pub inbox: VecDeque<Message>,
}

/// Thread manager - holds all threading state
struct ThreadManager {
    thread_table: BTreeMap<usize, Box<Thread>>,
    ready_queue: VecDeque<usize>,
    next_id: AtomicUsize,
}

impl ThreadManager {
    const fn new() -> Self {
        ThreadManager {
            thread_table: BTreeMap::new(),
            ready_queue: VecDeque::new(),
            next_id: AtomicUsize::new(1),
        }
    }

    fn allocate_thread_id(&self) -> usize {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Add a thread to the table
    fn add_thread(&mut self, thread: Box<Thread>) -> usize {
        let thread_id = thread.id;
        self.thread_table.insert(thread_id, thread);
        thread_id
    }

    /// Get a thread by ID
    fn get_thread(&mut self, thread_id: usize) -> Option<&mut Box<Thread>> {
        self.thread_table.get_mut(&thread_id)
    }

    /// Remove a thread from the table
    fn remove_thread(&mut self, thread_id: usize) -> Option<Box<Thread>> {
        self.thread_table.remove(&thread_id)
    }

    /// Add a thread to the back of the ready queue
    fn enqueue_ready(&mut self, thread_id: usize) {
        self.ready_queue.push_back(thread_id);
    }

    /// Add a thread to the front of the ready queue (priority wake)
    fn enqueue_ready_front(&mut self, thread_id: usize) {
        self.ready_queue.push_front(thread_id);
    }

    /// Get next ready thread from queue
    fn dequeue_ready(&mut self) -> Option<usize> {
        self.ready_queue.pop_front()
    }
}

// Global thread manager (protected by SpinLock for SMP safety)
static THREAD_MANAGER: Mutex<ThreadManager> = Mutex::new(ThreadManager::new());

// Current running thread (fast access without lock)
// For SMP: This would need to be per-CPU storage
static mut CURRENT_THREAD: *mut Thread = core::ptr::null_mut();

// Idle thread (not in thread table or ready queue — special scheduler fallback)
static mut IDLE_THREAD: *mut Thread = core::ptr::null_mut();
static mut IDLE_ENTRY: usize = 0;

impl Thread {
    /// Create a new kernel thread with the given entry point
    /// Returns Box<Thread> so the thread never moves after sp is calculated
    pub fn new(entry_point: fn()) -> Box<Self> {
        // Allocate stack on heap first (avoids stack overflow during construction)
        let stack = vec![0u8; STACK_SIZE];

        let thread_id = THREAD_MANAGER.lock().allocate_thread_id();

        let mut thread = Box::new(Thread {
            id: thread_id,
            state: ThreadState::Ready,
            context: ThreadContext::default(),
            stack,
            inbox: VecDeque::new(),
        });

        // Calculate sp based on the stable location of the Vec's buffer
        // Stack grows downward, so sp points to top
        let sp = thread.stack.as_ptr() as usize + STACK_SIZE;
        thread.context.sp = sp;
        thread.context.ra = entry_point as usize;

        thread
    }

    /// Get reference to the currently running thread
    pub fn current() -> &'static mut Thread {
        unsafe {
            assert!(!CURRENT_THREAD.is_null(), "No current thread");
            &mut *CURRENT_THREAD
        }
    }

    /// Set the current thread (used by scheduler during context switch)
    #[inline]
    pub fn set_current(thread: *mut Thread) {
        unsafe {
            CURRENT_THREAD = thread;
        }
    }
}

/// Register the idle thread. Called once during boot by kthread::idle::init().
/// The idle thread is not added to the thread table or ready queue.
pub fn set_idle_thread(thread: *mut Thread, entry: usize) {
    unsafe {
        IDLE_THREAD = thread;
        IDLE_ENTRY = entry;
    }
}

/// Public API for thread management

/// Add a thread to the thread table and ready queue
pub fn add(thread: Box<Thread>) -> usize {
    let mut manager = THREAD_MANAGER.lock();
    let thread_id = manager.add_thread(thread);
    manager.enqueue_ready(thread_id);
    thread_id
}

/// Helper: saves context assuming ra is already saved in t1
/// This is called by both save_context() and yield_thread()
/// Expects: t1 contains the original ra value to save
/// Modifies: t0 (used for context pointer)
/// Does NOT return - caller must handle return via ret instruction
#[naked]
unsafe extern "C" fn save_context_with_ra_in_t1() {
    core::arch::naked_asm!(
        // Get context pointer directly without function call
        "la t0, {current_thread}",
        "ld t0, 0(t0)",        // t0 = *CURRENT_THREAD
        "addi t0, t0, {context_offset}",

        // Save context - use t1 for ra (the ORIGINAL ra from caller)
        "sd sp, TC_SP(t0)",
        "sd t1, TC_RA(t0)",    // Save original ra from t1
        "sd s0, TC_S0(t0)",
        "sd s1, TC_S1(t0)",
        "sd s2, TC_S2(t0)",
        "sd s3, TC_S3(t0)",
        "sd s4, TC_S4(t0)",
        "sd s5, TC_S5(t0)",
        "sd s6, TC_S6(t0)",
        "sd s7, TC_S7(t0)",
        "sd s8, TC_S8(t0)",
        "sd s9, TC_S9(t0)",
        "sd s10, TC_S10(t0)",
        "sd s11, TC_S11(t0)",

        "ret",  // Return to caller

        current_thread = sym CURRENT_THREAD,
        context_offset = const core::mem::offset_of!(Thread, context),
    )
}

// Assembly function to restore context and jump to thread
// Takes a0 = pointer to ThreadContext
// Loads sp, ra, s0-s11 and executes ret (jumps to ra)
unsafe fn restore_context_asm(context: *const ThreadContext) -> ! {
    core::arch::asm!(
        "ld sp, TC_SP(a0)",
        "ld ra, TC_RA(a0)",
        "ld s0, TC_S0(a0)",
        "ld s1, TC_S1(a0)",
        "ld s2, TC_S2(a0)",
        "ld s3, TC_S3(a0)",
        "ld s4, TC_S4(a0)",
        "ld s5, TC_S5(a0)",
        "ld s6, TC_S6(a0)",
        "ld s7, TC_S7(a0)",
        "ld s8, TC_S8(a0)",
        "ld s9, TC_S9(a0)",
        "ld s10, TC_S10(a0)",
        "ld s11, TC_S11(a0)",
        "ret",  // Jump to restored ra (never returns)
        in("a0") context,
        options(noreturn)
    )
}

/// Yield the CPU to another thread
/// Saves current thread's context and schedules the next ready thread
/// Must preserve ALL callee-saved registers (s0-s11, sp, ra)
#[naked]
#[allow(dead_code)]
pub unsafe extern "C" fn yield_now() {
    core::arch::naked_asm!(
        // Save original ra to t1
        "mv t1, ra",

        // Call helper to save context (clobbers ra, but we have it in t1)
        "call {save_helper}",

        // Now call yield_impl to finish yielding
        // (ra gets clobbered but we don't care - we're not returning)
        "call {yield_impl}",

        save_helper = sym save_context_with_ra_in_t1,
        yield_impl = sym yield_impl,
    )
}

/// Implementation of yield after context is saved
#[allow(dead_code)]
fn yield_impl() -> ! {
    let current = Thread::current();
    let current_id = current.id;

    // Mark current thread as ready and add back to queue
    current.state = ThreadState::Ready;
    THREAD_MANAGER.lock().enqueue_ready(current_id);

    // Schedule next thread (never returns, but thread resumes here when rescheduled)
    schedule()
}

/// Scheduler - picks next ready thread and switches to it
/// Never returns - always jumps to a thread
fn schedule() -> ! {
    let mut manager = THREAD_MANAGER.lock();

    if let Some(next_id) = manager.dequeue_ready() {
        let next_thread = manager.get_thread(next_id).expect("Thread in ready queue not found");
        next_thread.state = ThreadState::Running;

        // Get raw pointer before dropping lock
        let next_ptr = next_thread.as_mut() as *mut Thread;

        // Drop lock before context switch (don't hold lock during thread execution)
        drop(manager);

        // Set as current and restore context (never returns)
        Thread::set_current(next_ptr);
        unsafe {
            restore_context_asm(&(*next_ptr).context as *const ThreadContext);
        }
    } else {
        // No ready threads — enter idle thread
        drop(manager);
        enter_idle();
    }
}

/// Start the scheduler - picks first ready thread and runs it
/// Call this after adding threads to the ready queue
/// Never returns
pub fn start_scheduler() -> ! {
    schedule()
}

/// Enter the idle thread. Resets sp to top of idle stack and jumps to idle_entry.
/// The idle thread is always restarted from its entry point (no context restore).
fn enter_idle() -> ! {
    unsafe {
        let idle_ptr = IDLE_THREAD;
        assert!(!idle_ptr.is_null(), "Idle thread not initialized");
        (*idle_ptr).state = ThreadState::Running;
        Thread::set_current(idle_ptr);

        let sp = (*idle_ptr).stack.as_ptr().add((*idle_ptr).stack.len()) as usize;
        core::arch::asm!(
            "mv sp, {sp}",
            "jr {entry}",
            sp = in(reg) sp,
            entry = in(reg) IDLE_ENTRY,
            options(noreturn),
        );
    }
}

/// Called by the idle loop after handling an interrupt.
/// If threads are ready, calls schedule() which never returns to idle.
/// If no threads are ready, returns so the idle loop can wfi again.
pub fn schedule_if_ready() {
    let has_ready = !THREAD_MANAGER.lock().ready_queue.is_empty();
    if has_ready {
        schedule();
    }
}

/// Exit the current thread
/// Removes thread from system and schedules next thread
/// Never returns
pub fn exit() -> ! {
    let current = Thread::current();
    let current_id = current.id;

    println!("Thread {} exiting", current_id);

    // Remove from thread table
    THREAD_MANAGER.lock().remove_thread(current_id);

    // Schedule next thread (current is no longer valid after this point)
    schedule()
}

/// Block the current thread (saves context and calls scheduler)
/// Unlike yield_now, does NOT enqueue the thread — it stays Blocked
/// until another thread calls send_message to wake it.
#[naked]
pub unsafe extern "C" fn block_now() {
    core::arch::naked_asm!(
        "mv t1, ra",
        "call {save_helper}",
        "call {block_impl}",
        save_helper = sym save_context_with_ra_in_t1,
        block_impl = sym block_impl,
    )
}

/// Implementation of block after context is saved
fn block_impl() -> ! {
    let current = Thread::current();

    current.state = ThreadState::Blocked;
    // Don't enqueue — send_message will wake us when a message arrives

    schedule()
}

/// Send a message to a target thread
/// If the target is Blocked, it is woken up and added to the ready queue
pub fn send_message(target_id: usize, msg: Message) {
    let mut manager = THREAD_MANAGER.lock();
    if let Some(target) = manager.get_thread(target_id) {
        target.inbox.push_back(msg);
        if target.state == ThreadState::Blocked {
            target.state = ThreadState::Ready;
            manager.enqueue_ready(target_id);
        }
    }
}

/// Wake a thread without sending a message.
/// Used for direct signaling (e.g., buffer space available) that doesn't
/// need to go through the message inbox.
pub fn wake_thread(target_id: usize) {
    let mut manager = THREAD_MANAGER.lock();
    if let Some(target) = manager.get_thread(target_id) {
        if target.state == ThreadState::Blocked {
            target.state = ThreadState::Ready;
            manager.enqueue_ready(target_id);
        }
    }
}

/// Send a message to a target thread and immediately yield to it.
/// The target is placed at the front of the ready queue, and the sender
/// yields so the target runs next. Use this for latency-sensitive receivers
/// like the UART writer or future interrupt handler threads.
///
/// Must be called as a naked function to save the caller's context before yielding.
#[naked]
pub unsafe extern "C" fn send_message_urgent(target_id: usize, msg_sender: usize, msg_data: usize) {
    core::arch::naked_asm!(
        "mv t1, ra",
        "call {save_helper}",
        "call {urgent_impl}",
        save_helper = sym save_context_with_ra_in_t1,
        urgent_impl = sym send_message_urgent_impl,
    )
}

/// Implementation after context is saved.
/// a0 = target_id, a1 = msg_sender, a2 = msg_data (preserved across save_context)
fn send_message_urgent_impl(target_id: usize, msg_sender: usize, msg_data: usize) -> ! {
    let current = Thread::current();
    let current_id = current.id;
    current.state = ThreadState::Ready;

    let mut manager = THREAD_MANAGER.lock();

    // Deliver the message
    if let Some(target) = manager.get_thread(target_id) {
        target.inbox.push_back(Message {
            sender: msg_sender,
            data: msg_data,
        });
        if target.state == ThreadState::Blocked {
            target.state = ThreadState::Ready;
            manager.enqueue_ready_front(target_id);
        }
    }

    // Re-enqueue sender to back of queue and yield
    manager.enqueue_ready(current_id);
    drop(manager);

    schedule()
}

/// Receive a message from the current thread's inbox
/// Blocks if the inbox is empty, yielding to the scheduler until a message arrives
pub fn receive_message() -> Message {
    loop {
        let current = Thread::current();
        if let Some(msg) = current.inbox.pop_front() {
            return msg;
        }
        // No message — block until sender wakes us
        unsafe { block_now(); }
    }
}

