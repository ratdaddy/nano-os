# Kernel Initialization Thread - Future Enhancement

## Problem

Currently, threads that depend on kernel subsystems being ready have no principled way to wait for initialization to complete. The workaround in `src/kthread/user_process.rs` polls for a specific block device (`blkdev_get(8, 0)`) as a proxy for "block init is complete":

```rust
// TODO: replace with a proper kernel-ready notification.
loop {
    match dev::blkdev_get(8, 0) {
        Ok(_) => break,
        Err(_) => unsafe { thread::yield_now() },
    }
}
```

This is fragile: it hardcodes a device number, only covers block init, and doesn't generalize to other subsystems (filesystems, network, etc.).

## Proposed Design: Kernel Init Thread

Introduce a dedicated kernel initialization thread that:

1. Runs at startup and sequentially initializes all kernel subsystems in dependency order
2. Sets a global "kernel ready" flag (or sends a broadcast) when all init is complete
3. Other threads that need a fully-initialized kernel block on this signal before proceeding

### Subsystem Init Ordering (example)

```
block drivers → disk detection → partition scan → VFS mount → device nodes → ready
```

### Notification Mechanism

Options (in increasing sophistication):

- **Atomic flag:** A `static AtomicBool KERNEL_READY` that threads spin on. Simple but wastes CPU.
- **Condvar / wait queue:** Threads block until the init thread signals. Better for power/scheduling.
- **Message broadcast:** Init thread sends a ready message to all waiting threads. Fits the existing message-passing model.

### Staged Init (longer term)

For fine-grained dependencies, subsystems could expose a readiness level rather than a single binary flag. Threads declare which level they require and block until that level is reached:

```
Level 0: basic memory/allocator
Level 1: block drivers
Level 2: disk + partitions
Level 3: VFS mounted
Level 4: device nodes (/dev, /proc)
Level 5: full userspace readiness
```

## Affected Code

- `src/kthread/user_process.rs` — replace poll loop with a proper wait on kernel-ready signal
- Any future subsystem init threads with similar polling patterns

## See Also

- Current workaround: `src/kthread/user_process.rs:66` (`user_thread_entry`)
