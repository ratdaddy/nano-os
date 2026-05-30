# Threading and Process Management Brief

A reference for discussing POSIX threading implementation based on current kernel infrastructure.
Current as of March 7, 2026

---

## Overview

The kernel uses a **cooperative scheduler** for kernel threads and **interrupt-driven execution** for user-mode processes. Threading is built around a simple round-robin ready queue with message-passing IPC.

---

## Core Data Structures

### Thread (`src/thread.rs:31`)

```rust
pub struct Thread {
    pub id: usize,
    pub state: ThreadState,           // Running | Ready | Blocked
    pub context: ThreadContext,       // Callee-saved regs (kernel context)
    pub stack: Vec<u8>,               // 32KB heap-allocated kernel stack
    pub inbox: VecDeque<Message>,     // IPC message queue
    pub process: Option<Box<process::Context>>,  // Present for user-mode threads
}
```

### ThreadContext (`types/src/lib.rs:99`)

Holds only callee-saved registers: `sp`, `ra`, `s0`–`s11`. Sufficient because context switches always happen at explicit yield/block points, never mid-instruction.

### Thread States

```
Ready ──► Running ──► Blocked
  ▲                      │
  └──────────────────────┘  (woken by message or wake_thread())
  ▲
  └── exit() removes thread entirely
```

### ThreadManager (`src/thread.rs:49`)

```rust
struct ThreadManager {
    thread_table: BTreeMap<usize, Box<Thread>>,
    ready_queue: VecDeque<usize>,   // FIFO, round-robin
    next_id: AtomicUsize,
}
```

Global singletons:
- `THREAD_MANAGER: Mutex<ThreadManager>` — authoritative state
- `CURRENT_THREAD: *mut Thread` — fast access, no lock needed for reads during execution
- `IDLE_THREAD: *mut Thread` — not in table/queue; fallback when nothing is ready

---

## Scheduler (`src/thread.rs:276`)

**Algorithm:** Round-robin (FIFO dequeue from ready_queue).

**Scheduling points** (no preemption timer exists):
1. `yield_now()` — explicit voluntary yield
2. `block_now()` — blocks waiting for a message
3. `exit()` — thread terminates
4. `schedule_if_ready()` — called from interrupt handlers and idle loop

When no threads are ready, the idle thread runs a `wfi` loop. Interrupts wake the CPU, and `schedule_if_ready()` reschedules if work has arrived.

**Lock discipline:** `THREAD_MANAGER` lock is released before the context switch itself, so the incoming thread does not inherit the lock.

**Trap handler configuration** happens in `schedule()` per thread: kernel threads use `kernel_trap_entry` + kernel scratch stack; user threads use `trap_entry` + trampoline trap frame.

---

## Context Switching (`src/thread.rs:180`)

Two naked assembly functions handle kernel thread switching:

**Save** (`save_context_with_ra_in_t1`): reads `CURRENT_THREAD`, saves `sp`, `s0`–`s11`, and `t1` (which holds the original `ra`) into `Thread.context`.

**Restore** (`restore_context_asm`): given a `*const ThreadContext`, loads all callee-saved registers and `ret`s to the saved `ra`. Does not return to caller.

**Yield sequence:**

```
yield_now() [naked]
  mv t1, ra          ← preserve return address
  call save_helper   ← saves context (uses t1 for ra)
  call yield_impl    ← picks next thread, restores its context
                        (never returns)
```

---

## User Processes

### Process Context (`src/process.rs:12`)

```rust
pub struct Context {
    pub page_map: PageMapper,                      // Sv39 page tables
    pub satp: usize,                               // Encoded root PPN
    pub heap_begin: usize,
    pub heap_end: usize,
    pub mmap_next: usize,
    pub trap_frame: &'static mut ProcessTrapFrame, // Lives on kernel stack
    pub files: Vec<Option<File>>,                  // File descriptor table
}
```

A user process is a `Thread` with `process: Some(...)`. The `Thread` struct owns the kernel stack; the `ProcessTrapFrame` (all 31 GP registers + sepc + satp) lives at the top of that kernel stack.

### Virtual Address Space Layout

| Region | Virtual Address |
|--------|----------------|
| ELF text/data | `0xffff_ffc0_0000_0000` |
| User stack (grows down) | `0xffe0_0000` (initial 16KB) |
| Anonymous mmap | `0x1_0000_0000` |
| Heap (via brk) | follows ELF load end |

### Spawning a Process (`src/kthread/user_process.rs:20`)

1. Open ELF file
2. Create `Thread` with entry `user_thread_entry`
3. Place `ProcessTrapFrame` at top of thread's kernel stack
4. `init_from_elf()` loads segments, sets `trap_frame.pc`/`sp`/`a0`/`a1`
5. Attach `process::Context` to thread; add to scheduler

When scheduled, `user_thread_entry()` calls `enter_process()`, which sets up `stvec`, `sscratch`, `satp`, `sepc`, adjusts `sstatus` (SPP=0, SPIE=1), then `sret`s into user mode.

---

## Trap Handling

### Kernel Trap (`src/kernel_trap.rs:34`)

Saves **all 31 GP registers** + `sepc` to a `KernelTrapFrame` on the dedicated trap stack (pointed to by `sscratch`). Used for kernel threads.

### User Trap (`src/trap.rs:10`)

Two-stage design:

1. **TrampolineTrapFrame** (mapped into user process VA space at a fixed address): minimal state—holds `kernel_sp`, `kernel_satp`, `user_sp`, `t0`. Pointed to by `sscratch` when in user mode.

2. On trap entry: swap `sp`/`sscratch` to get trampoline pointer, save `t0`/`user_sp`, switch SATP to kernel, load `kernel_sp`, save all GP registers + CSRs to `ProcessTrapFrame`, call `trap_handler`.

3. On return: restore registers, switch SATP back to user, `sret`.

### Trap Causes Handled

- `ECALL_FROM_U` → syscall dispatch (`src/syscall/mod.rs`)
- `EXTERNAL` interrupt → PLIC dispatch, device IRQ handlers
- `STORE_ACCESS_FAULT` → AMO emulation (for hardware lacking AMO support)
- Page faults → currently panic (no demand paging yet)

---

## IPC: Message Passing (`src/thread.rs:428`)

```rust
pub struct Message {
    pub sender: usize,
    pub data: usize,
}
```

- `send_message(id, msg)` — enqueues to target inbox; if target is Blocked, moves it to ready_queue back
- `send_message_urgent(id, msg)` — same but pushes target to **front** of ready_queue (priority wakeup) and yields to it immediately
- `receive_message() -> Message` — blocks if inbox empty; woken by sender
- `wake_thread(id)` — wakes without a message (used by interrupt handlers)

---

## Synchronization

### `spin::Mutex`

Used for `THREAD_MANAGER`. Single-core today so no contention, but correct for future SMP.

### `SpscRing<N>` (`src/collections/spsc_ring.rs:13`)

Lock-free single-producer single-consumer ring buffer. Uses `Acquire`/`Release` atomics on head/tail. Used for UART TX path: `kprint!` → writer thread → SPSC ring → TX interrupt drains to FIFO.

### Atomic flags

`AtomicBool`, `AtomicUsize` used for lightweight state (TX active, waiting for space, thread IDs).

---

## Memory Management

### Page Tables (`src/page_mapper.rs:175`)

`PageMapper` wraps an Sv39 3-level page table. Each process gets its own root table. The kernel is identity-mapped and shared across all address spaces (high half).

**Per-process allocations** (zeroed before mapping):
- ELF segments (copied from disk)
- Initial stack pages (`READ | WRITE | USER`)
- Heap growth via `brk` syscall
- Anonymous mappings via `mmap` syscall

---

## Key File Locations

| Component | File | Key Lines |
|-----------|------|-----------|
| Thread struct & states | `src/thread.rs` | 14–53 |
| Context save/restore asm | `src/thread.rs` | 180–256 |
| Scheduler | `src/thread.rs` | 276–340 |
| Message passing | `src/thread.rs` | 428–516 |
| Process context | `src/process.rs` | 12–53 |
| Spawn user process | `src/kthread/user_process.rs` | 20–56 |
| Enter user mode | `src/process_trampoline.rs` | 8–73 |
| ELF loader + VA layout | `src/process_memory_map.rs` | 16–149 |
| User trap entry (asm) | `src/trap.rs` | 10–144 |
| Kernel trap entry (asm) | `src/kernel_trap.rs` | 34–147 |
| Page mapper | `src/page_mapper.rs` | 175–284 |
| Syscall dispatch | `src/syscall/mod.rs` | 14–47 |
| Memory syscalls (brk/mmap) | `src/syscall/memory.rs` | 1–78 |
| SPSC ring | `src/collections/spsc_ring.rs` | 13–94 |
| Idle thread | `src/kthread/idle.rs` | 12–51 |
| ThreadContext type | `types/src/lib.rs` | 99–116 |
| Trap frame types | `types/src/lib.rs` | 3–94 |

---

## Syscall Inventory

### Currently Implemented

| Number | Name | File | State | Notes |
|--------|------|------|-------|-------|
| 25 | `fcntl` | `file.rs` | Stub — returns 0 | Needed for `FD_CLOEXEC` on `clone` |
| 56 | `openat` | `file.rs` | Working | |
| 57 | `close` | `file.rs` | Working | |
| 63 | `read` | `file.rs` | Working | |
| 64 | `write` | `file.rs` | Working (yields after write) | |
| 73 | `ppoll` | `signal.rs` | Stub — returns 0 | |
| 80 | `newfstat` | `file.rs` | Working | |
| 96 | `set_tid_address` | `process.rs` | Partial — returns kernel thread ID; does not store the `tidptr` | POSIX requires storing the address so the kernel can do `*tidptr = 0` + futex wake on thread exit |
| 132 | `sigaltstack` | `signal.rs` | Stub — returns 0 | |
| 134 | `rt_sigaction` | `signal.rs` | Stub — returns 0 | |
| 135 | `rt_sigprocmask` | `signal.rs` | Stub — returns 0 | |
| 178 | `gettid` | `process.rs` | Partial — returns kernel thread ID | Correct semantics once thread IDs are stable per-process |
| 214 | `brk` | `memory.rs` | Working (no guard, no shrink) | Needs mutex for shared-heap when multiple threads per process |
| 222 | `mmap` | `memory.rs` | Partial — anon private only, no hint support | Needs `MAP_FIXED`, `PROT_EXEC` for TLS/stack alloc; needs mutex for shared `mmap_next` |

### Missing — Required for `pthread_create` / NPTL

These syscalls are called by musl/glibc's `pthread_create` and related machinery. None are currently handled; they will hit the `Unhandled syscall` panic.

| Number | Name | Why Needed | Complexity |
|--------|------|-----------|------------|
| 93 | `exit` | Thread self-exit; must wake `set_tid_address` futex | Low — call `thread::exit()` |
| 94 | `exit_group` | Process-wide exit; kills all threads sharing the process | Medium — iterate threads with same process context |
| 98 | `futex` | `pthread_mutex`, `pthread_cond`, `pthread_join` all bottom out here | **High** — core primitive; needs `FUTEX_WAIT` / `FUTEX_WAKE` at minimum |
| 99 | `set_robust_list` | NPTL calls this on thread init; registers the per-thread robust futex list head | Low — store pointer, no kernel action needed until thread dies |
| 100 | `get_robust_list` | Complement to above | Low |
| 101 | `nanosleep` | Used in spin/backoff paths in libc | Medium — needs timer interrupt to wake sleeping thread |
| 124 | `sched_yield` | Explicit cooperative yield from user space | Low — call `thread::yield_now()` |
| 172 | `getpid` | Returns process ID; used by NPTL for `pthread_self` | Low — return process-level ID (not thread ID) |
| 215 | `munmap` | Free mmap'd regions; required to free thread stacks after join | Medium — need to track/unmap VA ranges |
| 220 | `clone` | **Core of `pthread_create`** — creates a new thread sharing VM + FDs | **Very High** — new kernel thread sharing `process::Context`; needs per-thread stacks, TLS (`tp`), new trap frame |
| 226 | `mprotect` | Used to set up stack guard pages for new threads | Medium — walk page table, update PTE flags |
| 260 | `wait4` / `waitid` | Waiting for child processes (less critical for pure threading) | Medium |

### Key Observations on Existing Stubs

**`set_tid_address` (96)** — `process.rs:3`

Currently just returns the thread ID but ignores the `tidptr` argument. The POSIX contract requires:
- Store the pointer in the thread's TCB
- On thread exit, write `0` to `*tidptr` and issue a `futex(tidptr, FUTEX_WAKE, 1)` so that `pthread_join` can wake up

**`signal` stubs (73, 132, 134, 135)**

Returning 0 is safe enough to get past libc init, but `rt_sigprocmask` in particular is called by NPTL to block signals during thread creation. This needs real semantics once signals are implemented.

**`mmap` (222)**

Three gaps that matter for threading:
1. `addr_hint != 0` is rejected — `clone` passes a specific stack address via `CLONE_VM`; the stack for the new thread is `mmap`'d by libc at a chosen address before calling `clone`
2. No `MAP_FIXED` support — required for TLS allocation
3. `mmap_next` is per-`process::Context` and not protected — two threads calling `mmap` simultaneously will race

**`brk` (214)**

`heap_end` on `process::Context` is unprotected. Fine today (one thread), but a mutex is needed once multiple threads share the same context.

---

## Gaps Relevant to POSIX Threading

Areas that will need new work or extension for `pthread`-style threading:

- **No shared address space between threads** — each `Thread` with a `process` has its own `PageMapper`. POSIX threads within a process share one address space; would require reference-counted or shared `PageMapper`.
- **No per-thread user stack allocation** — user stack is fixed at `0xffe0_0000`. Multiple threads need distinct stacks in the same VA space.
- **No thread-local storage (TLS)** — `tp` register is not saved/restored today.
- **No preemption timer** — POSIX threads are typically preemptible. Would require `stimecmp`/timer interrupt and a tick-based preemption mechanism.
- **ProcessTrapFrame lives on the kernel stack** — this works for one thread per process but needs revisiting for per-thread kernel stacks with multiple user threads in one process.
- **`files` / `heap` / `mmap` state is per-process-context** — for POSIX threads these are shared; mutexes will be needed around `brk`/`mmap`/fd allocation.
- **No futex or condvar primitive** — message passing covers kernel thread blocking, but `pthread_mutex` / `pthread_cond` want futex-style syscalls (`FUTEX_WAIT`, `FUTEX_WAKE`).
- **No `clone()` syscall** — Linux `pthread_create` uses `clone(CLONE_VM | CLONE_FILES | ...)`. Would need a new syscall that creates a thread sharing the current process context rather than forking it.
