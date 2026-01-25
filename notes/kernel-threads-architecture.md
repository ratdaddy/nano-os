# Kernel Threads Architecture (Design B)

This document describes the kernel threading model where driver work is performed by independent kernel threads that communicate with user processes via message passing.

## Core Concept

The system has two types of schedulable execution contexts:

```
┌─────────────────────────────────────────────────────┐
│ KERNEL THREADS                                       │
│ - Run in S-mode with kernel privileges              │
│ - Share kernel address space (same page table)      │
│ - Small fixed stacks (4-8 KB)                       │
│ - Minimal context (sp, ra, s0-s11 only)            │
│ - Never trap to higher privilege                    │
│ - Examples: UART driver, block device driver        │
└─────────────────────────────────────────────────────┘
           ↕ (Message passing)
┌─────────────────────────────────────────────────────┐
│ USER PROCESSES                                       │
│ - Run in U-mode, trap to S-mode for syscalls       │
│ - Isolated address spaces (separate page tables)   │
│ - Large dynamic stacks                              │
│ - Full context (all 31 registers + PC + status)    │
│ - Communicate via syscalls                          │
└─────────────────────────────────────────────────────┘
```

---

## Data Structures

### Kernel Thread

```rust
struct KernelThread {
    thread_id: usize,
    state: ThreadState,       // Running, Ready, Blocked

    // Minimal saved context (only for context switches)
    sp: usize,                // Stack pointer
    ra: usize,                // Return address
    s0_s11: [usize; 12],      // Callee-saved registers

    // Fixed-size stack
    stack: [u8; 8192],        // 8 KB stack

    // Message inbox
    inbox: VecDeque<Message>,

    // Optional: priority, name for debugging
}
```

**Why minimal context?**
- Context switches only happen at function call boundaries
- Compiler already saves caller-saved registers (t0-t6, a0-a7) on stack
- Only need to save callee-saved registers (s0-s11)
- Much faster than saving all 31 registers

### User Process

```rust
struct UserProcess {
    process_id: usize,
    state: ProcessState,      // Running, Ready, Blocked

    // Full trap frame (saved on syscall/interrupt)
    trap_frame: TrapFrame,    // All 31 registers + PC + status

    // Address space
    page_table: PageMapper,
    satp: usize,

    // Dynamic memory regions
    heap_start: usize,
    heap_end: usize,
    stack_top: usize,

    // Message inbox (for replies from kernel threads)
    inbox: VecDeque<Message>,

    // File descriptors, etc.
}
```

### Message

```rust
enum Message {
    Write {
        sender_thread_id: ThreadId,
        data: Vec<u8>,      // Data copied to kernel heap
        len: usize,
    },
    WriteComplete {
        bytes_written: usize,
    },
    DrainRequest,           // From interrupt handler
}
```

---

## Complete Example: printf("Hello\n") → UART

### Stage 1: User Space - printf()

**Executing Thread**: Process 5 (user process)
**CPU Mode**: U-mode
**Memory Map**: Process 5's page table
**Stack**: Process 5's user stack (e.g., 0x7fff_fff0)
**Interrupts**: Enabled (sstatus.SIE = 1)

**What happens:**
```c
// User code
printf("Hello\n");
```

Libc expands to:
1. Format string into buffer
2. Call `write(1, "Hello\n", 6)`
3. Setup syscall registers:
   - a7 = 64 (syscall number)
   - a0 = 1 (file descriptor - stdout)
   - a1 = &"Hello\n" (buffer pointer - user virtual address)
   - a2 = 6 (length)
4. Execute `ecall` instruction

**Transitions:**
- CPU mode: U-mode → S-mode (hardware transition)
- Interrupts: Enabled → Disabled (hardware clears sstatus.SIE)
- PC: User code → trap_entry
- Still process 5's thread!

---

### Stage 2: Trap Entry

**Executing Thread**: Process 5 (now in kernel mode)
**CPU Mode**: S-mode
**Memory Map**: Process page table → Kernel page table (transitions during this stage)
**Stack**: User stack → Kernel stack (transitions during this stage)
**Interrupts**: Disabled (sstatus.SIE = 0)

**What happens:**
Assembly code in `trap_entry`:
1. Swap sp with sscratch (get trampoline trap frame)
2. Save user t0 and sp to trampoline trap frame
3. Switch to kernel page table (csrw satp)
4. Flush caches (sfence.vma, T-Head dcache/icache)
5. Switch to process 5's kernel stack
6. Save all 31 registers + trap CSRs to ProcessTrapFrame
7. Call trap_handler()

**Key point**: This is still process 5's thread, just switched from user context to kernel context.

**State after trap_entry:**
- Thread: Process 5
- Mode: S-mode
- Memory: Kernel page table
- Stack: Process 5's kernel stack (0xffff_ff10_0050_0000)
- All user state saved in ProcessTrapFrame

---

### Stage 3: Trap Handler

**Executing Thread**: Process 5 (in kernel)
**CPU Mode**: S-mode
**Memory Map**: Kernel page table
**Stack**: Process 5's kernel stack
**Interrupts**: Disabled

**What happens:**
```rust
fn trap_handler(tf: &mut ProcessTrapFrame) -> usize {
    let scause = tf.scause;

    match scause {
        USER_ECALL => syscall::handle(tf),  // Dispatch to syscall handler
        // ... other trap types ...
    }

    tf as *const _ as usize
}
```

Examines `scause`, sees USER_ECALL (8), dispatches to syscall handler.

**Still process 5's thread executing in kernel mode.**

---

### Stage 4: Syscall Dispatcher

**Executing Thread**: Process 5 (in kernel)
**CPU Mode**: S-mode
**Memory Map**: Kernel page table
**Stack**: Process 5's kernel stack
**Interrupts**: Disabled

**What happens:**
```rust
fn syscall::handle(tf: &mut ProcessTrapFrame) {
    let syscall_num = tf.registers.a7;  // 64 (write)

    match syscall_num {
        64 => file::write(tf),
        214 => memory::brk(tf),
        222 => memory::mmap(tf),
        // ...
    }
}
```

Extracts syscall number from a7 (which is 64 for write), dispatches to `file::write()`.

**Still process 5's thread.**

---

### Stage 5: File Write Syscall

**Executing Thread**: Process 5 (in kernel)
**CPU Mode**: S-mode
**Memory Map**: Kernel page table
**Stack**: Process 5's kernel stack
**Interrupts**: Disabled

**What happens:**
```rust
pub fn file::write(tf: &mut ProcessTrapFrame) {
    let fd = tf.registers.a0;        // 1 (stdout)
    let user_buf = tf.registers.a1;  // User address 0x400080
    let len = tf.registers.a2;       // 6

    // Get current process context
    let ctx = process::Context::current();  // Process 5's context

    // Look up file descriptor
    let file = ctx.file_table.get(fd);  // Get FD 1 (stdout)

    // Copy data from user space to kernel space
    // (Must validate user_buf is in process 5's address space)
    let kernel_buf = [0u8; 6];
    copy_from_user(user_buf, &mut kernel_buf, len);
    // kernel_buf now contains "Hello\n"

    // Call file operations write method
    let bytes_written = file.ops.write(&kernel_buf, len);

    // Set return value
    tf.registers.a0 = bytes_written;
}
```

**Critical function**: `copy_from_user()`
- Temporarily switch back to process 5's page table
- Copy data from user address (0x400080) to kernel buffer
- Switch back to kernel page table
- Now data is in kernel memory, safe to pass around

**Still executing as process 5's thread in kernel mode.**

---

### Stage 6: Console File Operations - Message Sending

**Executing Thread**: Process 5 (in kernel)
**CPU Mode**: S-mode
**Memory Map**: Kernel page table
**Stack**: Process 5's kernel stack
**Interrupts**: Disabled

**What happens:**
```rust
impl ConsoleFileOps {
    fn write(&self, data: &[u8], len: usize) -> usize {
        // Prepare message for UART driver thread
        let msg = Message::Write {
            sender_thread_id: ThreadId::Process(5),  // Identify ourselves
            data: data.to_vec(),  // Copy data to kernel heap (persistent)
            len: len,
        };

        // Send message to UART driver kernel thread
        send_message(ThreadId::KernelThread(UART_DRIVER_THREAD), msg);

        // Block waiting for reply
        // THIS IS WHERE THREAD SWITCH HAPPENS
        let reply = receive_message();

        // When we return here, UART thread has sent reply
        match reply {
            Message::WriteComplete { bytes_written } => bytes_written,
            _ => 0,
        }
    }
}
```

**Message passing details:**
```rust
fn send_message(to: ThreadId, msg: Message) {
    let target_thread = THREAD_TABLE.get(to);

    // Add message to target's inbox
    target_thread.inbox.push(msg);

    // Wake target if it's blocked
    if target_thread.state == ThreadState::Blocked {
        target_thread.state = ThreadState::Ready;
        READY_QUEUE.push(target_thread);
    }
}

fn receive_message() -> Message {
    let current = current_thread();  // Process 5

    loop {
        // Check inbox
        if let Some(msg) = current.inbox.pop() {
            return msg;
        }

        // No message yet, block this thread
        current.state = ThreadState::Blocked;

        // CRITICAL: Call scheduler
        // This switches away from process 5
        schedule();

        // When we return here, we've been rescheduled
        // and someone sent us a message (that's why we woke)
    }
}
```

**What `schedule()` does:**
```rust
fn schedule() {
    // Save current thread's context
    let current = current_thread();
    save_context(current);  // Save sp, ra, s0-s11

    // Pick next ready thread
    let next = READY_QUEUE.pop();  // Could be UART thread, process 7, etc.

    // Restore next thread's context
    restore_context(next);  // Restore sp, ra, s0-s11

    // Return - but now we're executing as 'next' thread!
}
```

**CRITICAL TRANSITION:**
- Process 5's state: Running → Blocked
- Process 5's context saved (sp, ra, s0-s11 if kernel thread, or nothing if process)
- Scheduler picks next thread from READY_QUEUE
- Could be UART driver thread (likely, since we just woke it)
- Could be another user process
- Context switch happens here

---

### Stage 7: UART Driver Kernel Thread - Receives Message

**Executing Thread**: UART Driver Kernel Thread
**CPU Mode**: S-mode
**Memory Map**: Kernel page table (SAME as process 5 used)
**Stack**: UART thread's kernel stack (0xffff_ff10_0070_0000) - DIFFERENT!
**Interrupts**: Disabled (for now - could enable later)

**What happens:**
```rust
fn uart_driver_thread() {
    // This function runs forever as the UART driver thread
    let mut ring_buffer = RingBuffer::<8192>::new();

    loop {
        // Block waiting for messages
        let msg = receive_message();  // Returns when message arrives

        match msg {
            Message::Write { sender_thread_id, data, len } => {
                // Process write request
                for i in 0..len {
                    let byte = data[i];

                    // Add to ring buffer
                    while ring_buffer.is_full() {
                        // Buffer full, drain some to UART
                        drain_to_uart(&mut ring_buffer);

                        // Or yield to other threads
                        yield_thread();
                    }

                    ring_buffer.push(byte);
                }

                // Enable UART TX interrupt to drain buffer
                uart_enable_tx_interrupt();

                // Send reply to sender (process 5)
                send_message(sender_thread_id, Message::WriteComplete {
                    bytes_written: len,
                });
                // This makes process 5 READY again
            }

            Message::DrainRequest => {
                // From UART TX interrupt
                drain_to_uart(&mut ring_buffer);
            }
        }

        // Loop back to receive_message(), will block again
    }
}

fn drain_to_uart(buf: &mut RingBuffer) {
    // Drain up to 16 bytes (UART FIFO size)
    let mut count = 0;
    while count < 16 && !buf.is_empty() && uart_tx_ready() {
        let byte = buf.pop();
        uart_write_byte(byte);
        count += 1;
    }

    if buf.is_empty() {
        uart_disable_tx_interrupt();
    }
}
```

**What happens:**
1. UART thread was blocked in `receive_message()`
2. Process 5 sent message and woke UART thread (marked it READY)
3. Scheduler picked UART thread to run
4. UART thread receives message from inbox
5. UART thread adds "Hello\n" to ring buffer
6. UART thread enables TX interrupt
7. UART thread sends reply to process 5 (wakes process 5)
8. UART thread loops back to `receive_message()`, blocks again

**After sending reply:**
- Process 5 state: Blocked → Ready (because message arrived in inbox)
- UART thread state: Running → Blocked (waits for next message)
- Scheduler will eventually pick process 5 to run again

---

### Stage 8: Process 5 Resumes - Receives Reply

**Executing Thread**: Process 5 (in kernel)
**CPU Mode**: S-mode
**Memory Map**: Kernel page table
**Stack**: Process 5's kernel stack
**Interrupts**: Disabled

**When**: Scheduler picks process 5 from READY_QUEUE

**What happens:**
Process 5 was blocked in `receive_message()` inside `file::write()`.

```rust
fn receive_message() -> Message {
    let current = current_thread();

    loop {
        if let Some(msg) = current.inbox.pop() {
            return msg;  // ← We return here!
        }

        current.state = ThreadState::Blocked;
        schedule();
        // ↑ We were here when we blocked
        // ↓ We resume here when rescheduled
    }
}
```

Inbox now contains `Message::WriteComplete { bytes_written: 6 }`.

Returns to `file::write()`:
```rust
let reply = receive_message();  // Returns WriteComplete

match reply {
    Message::WriteComplete { bytes_written } => bytes_written,  // 6
}
// Returns 6
```

Returns to `syscall::handle()`, then to `trap_handler()`.

---

### Stage 9: Return to User Space

**Executing Thread**: Process 5 (in kernel, about to return to user)
**CPU Mode**: S-mode
**Memory Map**: Kernel page table
**Stack**: Process 5's kernel stack
**Interrupts**: Disabled

**What happens:**
trap_handler returns to trap_entry assembly (return path):
1. Restore all registers from ProcessTrapFrame
2. Restore sepc and sstatus
3. Switch to process page table (csrw satp)
4. Flush caches
5. Restore final registers (t0, sp) from trampoline trap frame
6. Execute `sret`

**Hardware performs (on sret):**
- PC ← sepc (return to user code after ecall)
- CPU mode ← U-mode
- Interrupts ← Enabled (sstatus.SIE ← sstatus.SPIE)

**Now back in user space:**
- Thread: Process 5
- Mode: U-mode
- Memory: Process 5's page table
- Stack: Process 5's user stack
- a0 register = 6 (return value from write syscall)

User's `write()` function returns 6.
User's `printf()` completes.

---

### Stage 10: UART TX Interrupt (Asynchronous)

**Executing Context**: INTERRUPT CONTEXT (not a thread!)
**CPU Mode**: S-mode
**Memory Map**: Whatever was active (could be any page table)
**Stack**: Current stack (whatever thread was running)
**Interrupts**: Disabled (hardware cleared on interrupt)

**When**: UART hardware triggers TX interrupt (FIFO empty)

**What happens:**
```rust
fn uart_tx_interrupt() {
    // We're in interrupt context - must be FAST
    // Can't block, can't yield, can't switch threads

    // Just send a message to UART thread (non-blocking)
    let result = send_message_nowait(
        ThreadId::KernelThread(UART_DRIVER_THREAD),
        Message::DrainRequest
    );

    // If send fails (queue full), that's okay - UART thread
    // will drain eventually

    // Complete PLIC interrupt
    plic::complete(uart_irq);

    // Return - resume whatever was interrupted
}

fn send_message_nowait(to: ThreadId, msg: Message) -> Result<(), QueueFull> {
    let target = THREAD_TABLE.get(to);

    if target.inbox.len() >= MAX_MESSAGES {
        return Err(QueueFull);
    }

    target.inbox.push(msg);

    if target.state == ThreadState::Blocked {
        target.state = ThreadState::Ready;
        READY_QUEUE.push(target);
    }

    Ok(())
}
```

**Key points:**
- Interrupt handler is NOT a thread
- Uses whatever stack was active (could be process 5's stack, UART thread's stack, etc.)
- Must be non-blocking (can't call schedule())
- Just queues work for UART thread and returns
- UART thread will be scheduled later to process DrainRequest

Later, when UART thread runs:
- Receives DrainRequest message
- Calls `drain_to_uart()` to push more bytes to UART
- Loops back to wait for more messages

---

## Thread States and Scheduling

### Thread States

```rust
enum ThreadState {
    Running,   // Currently executing on CPU
    Ready,     // Ready to run, in scheduler queue
    Blocked,   // Waiting for message or resource
}
```

### Scheduler

```rust
fn schedule() {
    // Save current thread's context
    let current = current_thread();
    save_context(current);

    // Mark current as not-running
    if current.state == ThreadState::Running {
        current.state = ThreadState::Ready;
        READY_QUEUE.push(current);
    }

    // Pick next thread to run
    let next = READY_QUEUE.pop().unwrap_or(IDLE_THREAD);
    next.state = ThreadState::Running;

    // Restore next thread's context
    restore_context(next);

    // Return - now executing as 'next' thread
}
```

**Context switch for kernel threads:**
```rust
fn save_context(thread: &mut KernelThread) {
    // Only save callee-saved registers
    unsafe {
        asm!(
            "sd sp, 0({thread})",
            "sd ra, 8({thread})",
            "sd s0, 16({thread})",
            // ... save s1-s11 ...
            thread = in(reg) &thread.sp,
        );
    }
}

fn restore_context(thread: &KernelThread) {
    // Only restore callee-saved registers
    unsafe {
        asm!(
            "ld sp, 0({thread})",
            "ld ra, 8({thread})",
            "ld s0, 16({thread})",
            // ... restore s1-s11 ...
            "ret",  // Jump to restored ra
            thread = in(reg) &thread.sp,
        );
    }
}
```

**Context switch for user processes:**
- Don't need to save/restore in `schedule()` - already in ProcessTrapFrame!
- Just track which process is running
- When returning from syscall, use ProcessTrapFrame to restore user state

---

## Summary of Thread Involvement

In this example, THREE execution contexts were involved:

### 1. Process 5 (User Process Thread)
- Started in user mode (printf)
- Trapped to kernel mode (syscall)
- Sent message to UART thread
- Blocked waiting for reply
- Was scheduled out
- Later: Was scheduled back in
- Received reply
- Returned to user mode

### 2. UART Driver (Kernel Thread)
- Was blocked waiting for messages
- Woke when process 5 sent message
- Was scheduled to run
- Processed write request
- Added data to ring buffer
- Sent reply to process 5
- Blocked again waiting for messages
- Later: Woke to handle interrupt's DrainRequest
- Drained buffer to UART

### 3. Interrupt Context (Not a Thread)
- UART TX interrupt fired
- Sent non-blocking message to UART thread
- Returned immediately

**Key insight**: Process 5 and UART thread are scheduled independently. Process 5 doesn't "wait busy" for UART - it truly blocks, allowing other work to proceed.

---

## Benefits of This Design

### 1. Clean Separation
- User processes don't directly manipulate hardware
- Drivers are isolated in kernel threads
- Clear ownership: UART thread owns the ring buffer

### 2. Blocking I/O Without Busy-Wait
- Process 5 blocks (gives up CPU)
- Other processes can run while UART drains
- Efficient use of CPU time

### 3. SMP Ready
- Multiple CPUs can run different threads simultaneously
- Process 5 could run on CPU 0
- UART thread could run on CPU 1
- Same design works for 1 or N CPUs

### 4. Simpler Driver Code
- Driver written in blocking style (natural)
- No complex state machines
- `receive_message()` → process → `send_reply()` → loop

### 5. Debuggable
- Can log message traffic
- Can see which thread is blocked on what
- Clear execution flow

---

## Memory and Stack Layout

### Kernel Memory (Shared by All Kernel Threads)

```
High Memory:
┌─────────────────────────────────────┐
│ Kernel Code (.text)                 │  RX
├─────────────────────────────────────┤
│ Kernel Data (.data, .rodata)        │  RW / R
├─────────────────────────────────────┤
│ Kernel Heap                         │  RW
│  - Ring buffers                     │
│  - Message queues                   │
│  - Thread structures                │
├─────────────────────────────────────┤
│ UART Thread Stack (8 KB)            │  RW
├─────────────────────────────────────┤
│ Process 5 Kernel Stack (8 KB)       │  RW
├─────────────────────────────────────┤
│ Process 7 Kernel Stack (8 KB)       │  RW
├─────────────────────────────────────┤
│ ... more process kernel stacks ...  │
└─────────────────────────────────────┘
```

All kernel threads see this same memory (same page table).

### Process 5 Memory (Private)

```
Low Memory (Process 5's Page Table):
┌─────────────────────────────────────┐
│ User Code                           │  0x00010000
├─────────────────────────────────────┤
│ User Data                           │  0x00020000
├─────────────────────────────────────┤
│ User Heap                           │  0x00100000
│  ...                                │
├─────────────────────────────────────┤
│ User Stack                          │  0x7ffff000
│  ↓ grows down                       │
└─────────────────────────────────────┘

High Memory (mapped in Process 5's Page Table):
┌─────────────────────────────────────┐
│ Kernel Code/Data/Stacks             │  0xffff_ff...
│ (same as kernel page table)         │  (read-only or inaccessible from user)
└─────────────────────────────────────┘
```

Process 5 can't access kernel memory from user mode (would page fault).

---

## Comparison to Current Design

### Current (Non-Threaded)

**Syscall execution:**
- Process 5 traps to kernel
- Runs syscall handler (as process 5 in kernel mode)
- Directly writes to UART ring buffer
- If buffer full: spins or blocks (yields to scheduler)
- Returns to user

**Interrupt:**
- Interrupt fires
- Drains ring buffer directly
- Returns

**Threads involved:** Just user processes (1 thread)

### New (Kernel Thread)

**Syscall execution:**
- Process 5 traps to kernel
- Sends message to UART kernel thread
- Blocks waiting for reply
- UART thread processes request
- Sends reply, process 5 wakes
- Returns to user

**Interrupt:**
- Interrupt fires
- Sends message to UART kernel thread
- Returns
- UART thread wakes, drains buffer

**Threads involved:** User processes + kernel threads (2+ threads)

---

## Next Steps

To implement this design:

1. **Add kernel thread support**
   - KernelThread structure
   - Context switch code (save/restore sp, ra, s0-s11)
   - Thread creation/initialization

2. **Add message passing**
   - Message structure
   - Per-thread inbox (VecDeque or ring buffer)
   - send_message() / receive_message() functions

3. **Enhance scheduler**
   - Track both user processes and kernel threads
   - Schedule both types (can use same queue or separate)

4. **Create UART driver thread**
   - Entry function (uart_driver_thread)
   - Initialization (create thread, start running)
   - Message handling loop

5. **Modify syscall handlers**
   - Change from direct I/O to message passing
   - copy_from_user() to move data to kernel
   - send_message() + receive_message() pattern

6. **Update interrupt handlers**
   - Change from direct work to sending messages
   - Non-blocking message send

This architecture provides a clean foundation for SMP support and allows driver code to be written in a simple, blocking style while maintaining system responsiveness.

---

## Hardware-Specific Considerations: T-Head C906 (NanoRV)

The T-Head C906 processor (used in LicheeRV Nano) has specific memory configuration requirements that affect kernel threading.

### Memory Attribute Extension (XTheadMae)

The C906 uses custom PTE bits 59-63 for memory attributes when MAEE (Memory Attribute Extension Enable) is set in `mxstatus`:

| Bit | Name | Description |
|-----|------|-------------|
| 63 | SO | Strong Order - for MMIO |
| 62 | C | Cacheable |
| 61 | B | Bufferable |
| 60 | SH | Shareable |
| 59 | Sec | Secure/Trustable |

### Atomic Operations Require Cacheable Memory

**Critical**: The C906 requires memory to be marked Cacheable (C=1) for AMO (Atomic Memory Operations) to work. Without this flag, atomic operations trigger:
- Exception code 7: Store/AMO access fault

This affects:
- `spin::Mutex` (uses `amoor.w.aq` for locking)
- `AtomicUsize`, `AtomicU8` operations
- Any synchronization primitives

**Solution**: All memory regions where atomics may execute must have `THEAD_MEMORY` flags:

```rust
// In page_mapper.rs
pub const THEAD_MEMORY: Self = Self { bits: 0x0Fusize << 59 };  // C=1, B=1, SH=1, Sec=1
```

Apply to: `.data`, `.bss`, kernel stack, kernel heap, trap frames.

### Cache Management After Page Table Modifications

After modifying page tables (e.g., growing heap or stack), the C906 requires explicit cache management:

```rust
// 1. Map new pages with THEAD_MEMORY flags
mapper.allocate_and_map_pages(addr, size, flags | PageFlags::THEAD_MEMORY);

// 2. Clean and invalidate D-cache
unsafe {
    core::arch::asm!(
        ".long 0x0030000b",   // th.dcache.ciall
        options(nostack, preserves_flags),
    );
}

// 3. Flush TLB
unsafe {
    core::arch::asm!("sfence.vma zero, zero", options(nostack, preserves_flags));
}
```

**Cache instruction options**:
- `0x0020000b`: `th.dcache.call` - Clean (writeback) all D-cache
- `0x0010000b`: `th.dcache.iall` - Invalidate all D-cache (discards dirty data!)
- `0x0030000b`: `th.dcache.ciall` - Clean AND invalidate all D-cache (**recommended**)
- `0x0100000b`: `th.icache.iall` - Invalidate all I-cache

**Important**: Use `dcache.ciall` (not just `dcache.call`) to avoid return address corruption during heap growth operations.

### MMIO Requires Strong Ordering

Device memory (UART, PLIC) must use Strong Order flag to prevent out-of-sequence access:

```rust
pub const THEAD_SO: Self = Self { bits: 1usize << 63 };
```

Without SO, PLIC interrupt claims may return zero and UART reads may miss data.

### Summary for Thread Implementation

When implementing kernel threads on C906:

1. **Thread stacks**: Map with `THEAD_MEMORY` flag for atomic operations in Mutex
2. **Thread table/manager**: Lives in `.data`/`.bss` which must have `THEAD_MEMORY`
3. **Heap growth**: Use `dcache.ciall` + `sfence.vma` sequence
4. **Stack growth**: Same cache flush sequence as heap

See `notes/thead-c906-memory-guide.md` for complete documentation.
