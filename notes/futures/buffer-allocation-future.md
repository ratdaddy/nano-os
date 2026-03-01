# Buffer Allocation Improvements (Future Work)

> **Superseded by `plans/block-cache-plan.md`**
> The block cache plan addresses all the concerns raised here. The aligned
> heap allocation approach (Option B) is already in use in `DirEntryIter`.
> `MBR_BUFFER` in `src/block/init.rs` remains out of scope for that plan.

## Current State

Block I/O currently uses static buffers allocated with `#[repr(C, align(512))]`:
- `src/block/init.rs`: `MBR_BUFFER`, `SECTOR_BUFFER`
- These are static to ensure proper DMA alignment

## Problems

1. **Stack pressure** - Static buffers contribute to thread stack usage
2. **Not scalable** - Can't easily allocate multiple buffers dynamically
3. **Static everywhere** - Each subsystem needs its own static buffer
4. **DMA alignment concerns** - Need to ensure heap-allocated buffers are properly aligned

## Proposed Solution

Create a heap-allocated aligned buffer type:

```rust
// Option A: Trust Box with verification
#[repr(C, align(512))]
pub struct AlignedBuffer([u8; 512]);

impl AlignedBuffer {
    pub fn new() -> Box<Self> {
        let buf = Box::new(AlignedBuffer([0; 512]));
        // Runtime verification
        assert_eq!(buf.as_ptr() as usize % 512, 0, "Buffer not properly aligned");
        buf
    }
}

// Option B: Explicit alloc primitives
use alloc::alloc::{alloc, dealloc, Layout};

pub struct AlignedBuffer {
    ptr: NonNull<[u8; 512]>,
}

impl AlignedBuffer {
    pub fn new() -> Self {
        let layout = Layout::from_size_align(512, 512).unwrap();
        let ptr = unsafe { alloc(layout) };
        assert!(!ptr.is_null());
        // ... initialize and return
    }
}
```

## Verification Strategy

Before deploying aligned heap buffers:

1. **Add allocator debugging** - Add print statements to `kernel_allocator` to log:
   - Allocation requests with size and alignment
   - Returned pointer addresses
   - Verify alignment is respected

2. **Test allocation** - Create test buffer and verify:
   ```rust
   let buf = AlignedBuffer::new();
   assert_eq!(buf.as_ptr() as usize % 512, 0);
   ```

3. **DMA test** - Use buffer for actual block I/O to ensure hardware accepts it

## When to Implement

This work should be done when implementing:
- **Multi-buffer reads** - Need to allocate multiple buffers dynamically
- **Buffer pools** - Pre-allocate buffers for performance
- **Filesystem caching** - Need many buffers for cache pages

## Benefits

- Reduces stack pressure on threads
- Enables dynamic buffer allocation
- Cleaner code (no static buffers everywhere)
- Scales to multiple concurrent I/O operations

## Files to Update

- New: `src/block/buffer.rs` - AlignedBuffer implementation
- Update: `src/block/init.rs` - Use AlignedBuffer instead of static buffers
- Update: `src/block/disk/mod.rs` - Consider buffer pooling in read_blocks()
- Update: `src/kernel_allocator/` - Add debug logging (optional, for verification)
