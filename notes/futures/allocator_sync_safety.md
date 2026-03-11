# Allocator UnsafeCell and Sync safety

## Current situation

`LinkedListAllocator` uses `UnsafeCell` for its four internal pointers (`head`, `tail`,
`heap_end`, `free_head`) because `GlobalAlloc` requires `&self` on `alloc`/`dealloc`.
`unsafe impl Sync` is asserted manually.

This is sound under the current invariant: interrupts are only enabled in the idle thread
(which does not allocate) and briefly during thread yields (not mid-allocation). On a
single CPU this guarantees the allocator is never re-entered.

## Conditions that would break the current approach

1. **Interrupt handlers that allocate** — if an ISR fires mid-`find_fit` or
   `split_block` and calls `alloc`, the free list would be corrupted. Currently safe
   because no such ISR exists and interrupts are disabled during all kernel allocation
   paths.

2. **SMP** — two CPUs could enter `alloc` simultaneously regardless of interrupt state.

## What to do when either condition arises

Move the four pointer fields into an inner struct and wrap it with `spin::Mutex`:

```rust
struct AllocatorInner {
    head:     *mut BlockHeader,
    tail:     *mut BlockHeader,
    heap_end: *mut BlockHeader,
    free_head: *mut BlockHeader,
}

// Safety: pointers refer only to the allocator's own heap region.
unsafe impl Send for AllocatorInner {}

pub struct LinkedListAllocator {
    inner: Mutex<AllocatorInner>,
    grow_heap_fn: fn(usize) -> Option<(usize, usize)>,
}
```

- `unsafe impl Sync` disappears — `Mutex<T: Send>` is `Sync` automatically.
- All four `UnsafeCell` fields disappear.
- `unsafe impl Send for AllocatorInner` is the remaining honest `unsafe`: asserting
  that the raw pointers are safe to move between CPUs because they belong to the
  allocator's private heap.

## Interrupt + spinlock interaction

A spinlock alone is not sufficient if ISRs can allocate. On the same CPU, acquiring a
spinlock in normal code and then taking an interrupt that also tries to acquire the same
lock will deadlock (the CPU spins forever waiting for a lock it already holds). The
correct pattern is to disable interrupts before acquiring the allocator lock:

```rust
let _irq = disable_interrupts_guard(); // re-enables on drop
let mut inner = self.inner.lock();
```

This is only needed if/when interrupt handlers are permitted to allocate.
