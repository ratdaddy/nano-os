# Refactor Brief: Thread → Task, Shared Process, and Per‑Task User Context

## Purpose

Refactor the current kernel threading and process model so a single process can support multiple independently schedulable execution contexts in the same address space.

This is the architectural prerequisite for:

* `clone()`-style thread creation
* POSIX-style multithreading
* shared process resources across threads
* correct per-thread user register state
* future futex-based synchronization

The core design change is:

> The scheduler runs **Tasks**.
> A **Process** is a shared resource container.
> A user-capable Task may execute in a Process context, but the Process itself is not an execution context.

---

# Current Model Summary

Today the kernel effectively models:

* a schedulable `Thread`
* which may optionally own a `process::Context`
* where `process::Context` contains both:

  * shared process-like resources
  * per-thread user execution state (`ProcessTrapFrame`)

This works for the current one-thread-per-process design, but it conflates two concepts:

1. shared process resources
2. per-thread execution state

That conflation becomes invalid once multiple threads exist in the same process.

---

# Architectural Goals

The refactor should establish the following model.

## 1. Task

The unit the scheduler runs.

A Task always has:

* kernel scheduling state
* a kernel stack
* ready / blocked / running state
* task identity (`tid`)
* inbox / IPC state (if retained)

A Task may additionally be **user-capable**, in which case it also has:

* a per-task user trap frame
* a per-task TLS / thread pointer value
* a reference to a shared `Process`

---

## 2. Process

A shared container for process-wide resources.

A Process owns:

* address space / page tables
* heap / `brk` state
* `mmap` allocator state
* file descriptor table
* signal dispositions (future)
* process identity (`pid` / thread-group id)
* membership list of Tasks in the process

A Process does **not** own register state.

---

## 3. Per‑Task User Continuation

Each user-capable Task has its own full saved user register state.

This is the current `ProcessTrapFrame`, but it must become **Task-owned**, not `Process`-owned.

---

# Core Conceptual Model

## Kernel continuation vs user continuation

There are two distinct continuation types in the kernel.

### Kernel continuation

This is the current `ThreadContext`.

It stores the minimal callee-saved register state needed to resume kernel execution after a voluntary yield or block.

It answers:

> Where does this Task resume in kernel mode?

This is **not a full CPU snapshot**.

---

### User continuation

This is the current `ProcessTrapFrame`.

It stores full user register state plus control state needed to return to user mode using `sret`.

It answers:

> Where does this Task resume in user mode?

This **is a full user CPU snapshot**.

---

## Rule

* `ThreadContext` resumes **kernel execution**
* `ProcessTrapFrame` resumes **user execution**
* `Process` owns **shared resources**, not register state

---

# Proposed Data Model

## Task

Rename `Thread` → `Task`.

Conceptual shape:

```rust
pub struct Task {
    pub id: usize,
    pub state: TaskState,
    pub context: TaskContext,
    pub stack: Vec<u8>,
    pub inbox: VecDeque<Message>,

    pub user: Option<UserTaskState>,
}
```

Where:

```rust
pub struct UserTaskState {
    pub process: Arc<Process>,
    pub trap_frame: *mut ProcessTrapFrame,
    pub tls_tp: usize,
}
```

Notes:

* `trap_frame` is **per Task**
* `tls_tp` is **per Task**
* `process` is **shared** via `Arc`

---

## Process

Refactor `process::Context` into a shared `Process` object.

Conceptual shape:

```rust
pub struct Process {
    pub pid: usize,

    pub page_map: Arc<PageMapper>,
    pub satp: usize,

    pub heap: Mutex<HeapState>,
    pub mmap: Mutex<MmapState>,
    pub files: Mutex<Vec<Option<File>>>,

    pub tasks: Mutex<Vec<usize>>,
}
```

Supporting structs:

```rust
pub struct HeapState {
    pub heap_begin: usize,
    pub heap_end: usize,
}

pub struct MmapState {
    pub mmap_next: usize,
}
```

Notes:

* `files`, `heap`, and `mmap` must be synchronized
* `page_map` is shared across Tasks
* `Process` must not contain trap frames

---

# IDs and Grouping

The kernel should follow this model:

* each Task has a **TID**
* each Process has a **PID**
* all Tasks in a Process share the same PID

Example:

```
Process PID 42
 ├── Task TID 42
 ├── Task TID 43
 └── Task TID 44
```

This supports:

* `getpid()`
* `gettid()`
* `exit_group()`
* thread groups

---

# Trap Frame Ownership and Storage

## Current behavior

Today the `ProcessTrapFrame`:

* lives physically on the kernel stack
* is referenced from `process::Context`

This works only because one thread exists per process.

---

## Correct model

The trap frame may continue to **live on the kernel stack**, but it must belong to the **Task**.

Rule:

> Storage location is independent of ownership.

Valid design:

* `ProcessTrapFrame` stored at the top of each Task's kernel stack
* Task stores pointer or derives its location

Invalid design after multithreading:

* `Process` storing a trap frame pointer

---

# Trap Relay Renaming

`TrampolineTrapFrame` is poorly named.

It is not a trap frame but a transition structure used during trap entry.

Recommended rename:

```
TrampolineTrapFrame → TrapRelay
```

This reflects its role relaying control between user and kernel trap handling.

---

# KernelTrapFrame Question

The current kernel trap frame may only be used by the idle thread.

However removal should **not** be part of this refactor.

Reasons:

* unrelated to Task/Process split
* trap path stability is critical
* future interrupt or preemption work may require it

Recommendation:

Leave `KernelTrapFrame` unchanged during this refactor.

Re-evaluate afterward.

---

# Scheduling Model Impact

The scheduler continues to run **Tasks**.

No fundamental scheduling changes are required.

Key rule:

> The scheduler schedules Tasks, not Processes.

Blocking, waking, and yielding operate on Tasks.

---

# Shared State Synchronization

Once multiple Tasks share a Process, the following become shared mutable state:

* heap (`brk`)
* mmap allocator
* file descriptor table

These must be protected with locks.

---

# TLS / Thread Pointer

RISC-V uses the `tp` register for thread-local storage.

Each Task must maintain its own `tp`.

Requirements:

* store TLS pointer per Task
* restore on user entry
* initialize on clone

---

# Immediate Architectural Consequences

After the refactor:

True statements:

* A Process can contain multiple Tasks
* Tasks share Process resources
* Each Task has its own trap frame

No longer true:

* A Process has register state
* A Process is equivalent to a single thread

---

# Refactor Plan

## Phase 1 — Naming

Rename core types:

```
Thread → Task
ThreadManager → TaskManager
CURRENT_THREAD → CURRENT_TASK
IDLE_THREAD → IDLE_TASK
```

---

## Phase 2 — Split process::Context

Move shared state into `Process`.

Remove trap frame from process.

Introduce `UserTaskState` attached to Task.

---

## Phase 3 — Move TrapFrame Ownership

Remove trap frame from Process.

Associate trap frame with Task.

Continue storing it on the kernel stack.

---

## Phase 4 — Update Process Spawn

User process creation should:

1. allocate Task
2. allocate kernel stack
3. place trap frame on stack
4. create Process
5. attach `UserTaskState`
6. initialize trap frame registers

---

## Phase 5 — Update Trap Handling

Trap entry must operate using the **current Task's trap frame**, not a Process-owned frame.

---

## Phase 6 — Process Membership

Process tracks member Tasks.

This enables:

* `exit_group()`
* process lifecycle

---

## Phase 7 — Synchronize Shared State

Add locks around:

* heap
* mmap
* fd table

---

## Phase 8 — Prepare for clone()

`clone()` will:

1. allocate new Task
2. allocate kernel stack
3. create trap frame
4. share Process (`Arc`)
5. initialize registers
6. enqueue Task

---

# Resulting Architecture

Scheduler → runs Tasks

Task → execution context

Process → shared resource container

---

# Terminology

Use **Task** for:

* scheduler entity
* kernel worker
* user execution context

Use **Process** for:

* shared resources
* address space

---

# Non‑Goals

Out of scope for this refactor:

* full signal semantics
* preemption
* futex implementation
* clone implementation itself
* robust futex lists

These will follow once the structural separation exists.

---

# Final Invariants

After refactor:

1. Scheduler runs Tasks
2. Process owns shared resources
3. Trap frames belong to Tasks
4. Multiple Tasks can share one Process
5. Shared state is synchronized

---

# Summary

Execution belongs to **Tasks**.

Shared resources belong to **Processes**.

Separating these concerns enables correct multithreading semantics and simplifies future implementation of clone, futex, and POSIX threading.
