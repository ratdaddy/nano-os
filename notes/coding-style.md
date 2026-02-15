# Coding Style and Conventions

## Import Organization

**Principle:** All imports at the top, qualified names at call sites.

### Structure

1. External crate imports (alloc, core, spin, etc.)
2. Blank line
3. Internal crate imports (all using `use crate::`)
4. Never use `crate::` at call sites - only in import statements

### Example

```rust
// External crates first
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

// Blank line separator
use crate::block::{dispatcher, BlockDevice, BlockError};
use crate::drivers::{plic, sd, virtio_blk};
use crate::dtb;
use crate::thread;

// Then use qualified names at call sites
fn example() {
    let tid = thread::Thread::current().id;
    let device = virtio_blk::init()?;
    dispatcher::send_read_completion(Ok(()));
}
```

**Bad:**
```rust
use crate::thread;

fn example() {
    crate::block::dispatcher::send_read_completion(Ok(()));  // Don't do this
}
```

**Good:**
```rust
use crate::block::dispatcher;
use crate::thread;

fn example() {
    dispatcher::send_read_completion(Ok(()));  // Do this
}
```

## Static Mutable References

**Principle:** Use `&raw mut` pattern to avoid undefined behavior warnings.

Rust 2024 edition discourages creating mutable references to mutable statics. Use raw pointers instead.

### Pattern

```rust
static mut MY_STATIC: Option<Data> = None;

// Bad - creates mutable reference
unsafe {
    MY_STATIC.take();
    MY_STATIC = Some(value);
}

// Good - use raw pointer
unsafe {
    let ptr = &raw mut MY_STATIC;
    (*ptr).take();
    *ptr = Some(value);
}
```

### Buffer Access

```rust
static mut BUFFER: [u8; 512] = [0; 512];

// Bad
unsafe {
    let buf = &mut BUFFER;
    process(buf);
}

// Good
unsafe {
    let buf = &raw mut BUFFER;
    let buf = &mut *buf;
    process(buf);
}
```

## Unsafe Blocks

**Principle:** Don't nest `unsafe` blocks unnecessarily.

### Pattern

```rust
// Bad - nested unsafe blocks
unsafe {
    let data = &raw mut STATIC_DATA;
    let result = unsafe {  // Unnecessary nested unsafe
        some_unsafe_operation()
    };
}

// Good - single unsafe block
unsafe {
    let data = &raw mut STATIC_DATA;
    let result = some_unsafe_operation();
}
```

## Naming Conventions

### Async vs Sync Methods

Use method names that clearly indicate whether operations are asynchronous (non-blocking) or synchronous (blocking).

- **Async operations:** Prefix with `start_` to indicate they return immediately
  - `BlockDriver::start_read()` - starts DMA, returns immediately

- **Sync operations:** Use simple verb forms for operations that block until complete
  - `BlockDisk::read_blocks()` - blocks until read completes (future)
  - `BlockVolume::read_blocks()` - blocks until read completes (future)

### Singular vs Plural

- Use singular when method handles exactly one item with fixed-size buffer
  - Current: `start_read(sector, buf: &mut [u8; 512])`

- Use plural when method can handle multiple items or variable-size buffer
  - Future: `read_blocks(lba, dst: &mut [u8])` - supports multi-block

- Keep plural names even if current implementation only supports single operations, if the design intends to support multiple items in the future

## File Organization

### Modified Files Policy

When making changes, check git status and ensure consistency across all modified files:

```bash
git status --short
```

Apply the same style principles to all files being changed in a commit.

## Code Review Checklist

Before committing:

- [ ] All imports follow the external → blank → internal pattern
- [ ] No `crate::` usage at call sites (only in imports)
- [ ] Mutable statics use `&raw mut` pattern
- [ ] No unnecessary nested `unsafe` blocks
- [ ] Method names clearly indicate async vs sync behavior
- [ ] All modified files follow consistent style
