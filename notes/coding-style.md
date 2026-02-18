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

## Magic Numbers and Named Constants

**Principle:** Replace magic numbers with appropriately named constants.

Magic numbers make code harder to understand and maintain. Use named constants that convey meaning and context.

### What to Replace

- **Sizes and counts:** Block sizes, buffer sizes, array lengths, iteration limits
- **Offsets:** Structure field offsets, header positions, table locations
- **Signatures and magic values:** File format identifiers, checksums, special markers
- **Hardware values:** Register addresses, device IDs, timing constants

### Choosing Constant Names

Match the constant name to its specific use, not just its numeric value:

```rust
// Bad - ambiguous names
const SIZE: usize = 512;
const OFFSET: usize = 446;

// Good - names reflect meaning and context
const BLOCK_SIZE: usize = 512;  // When referring to block I/O
const SECTOR_SIZE: u64 = 512;   // When referring to disk geometry
const MBR_PARTITION_TABLE_OFFSET: usize = 446;
const MBR_SIGNATURE: u16 = 0xAA55;
const BYTES_PER_MB: u64 = 1024 * 1024;
```

### Examples

```rust
// Bad - what do these numbers mean?
if block0[510] == 0x55 && block0[511] == 0xAA {
    for i in 0..4 {
        let offset = 446 + i * 16;
        let size_mb = num_sectors * 512 / (1024 * 1024);
    }
}

// Good - clear and self-documenting
const MBR_SIGNATURE: u16 = 0xAA55;
const MBR_SIGNATURE_OFFSET: usize = 510;
const MBR_PARTITION_TABLE_OFFSET: usize = 446;
const MBR_PARTITION_ENTRY_SIZE: usize = 16;
const SECTOR_SIZE: u64 = 512;
const BYTES_PER_MB: u64 = 1024 * 1024;

let signature = u16::from_le_bytes(
    block0[MBR_SIGNATURE_OFFSET..MBR_SIGNATURE_OFFSET + 2]
        .try_into().unwrap()
);
if signature == MBR_SIGNATURE {
    for i in 0..4 {
        let offset = MBR_PARTITION_TABLE_OFFSET + i * MBR_PARTITION_ENTRY_SIZE;
        let size_mb = num_sectors * SECTOR_SIZE / BYTES_PER_MB;
    }
}
```

### Scope and Placement

- Module-level constants for values used within that module
- Public constants (via re-export or pub const) for values shared across modules
- Group related constants together with comments

```rust
// MBR partition table layout
const MBR_SIGNATURE: u16 = 0xAA55;
const MBR_SIGNATURE_OFFSET: usize = 510;
const MBR_PARTITION_TABLE_OFFSET: usize = 446;
const MBR_PARTITION_ENTRY_SIZE: usize = 16;

// Boot sector detection
const BOOT_SIGNATURE: u16 = 0xAA55;
const BOOT_SIGNATURE_OFFSET: usize = 510;
const FAT32_FILESYSTEM_TYPE_OFFSET: usize = 82;
const FAT32_VOLUME_LABEL_OFFSET: usize = 71;
```

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
- [ ] Magic numbers replaced with appropriately named constants
- [ ] All modified files follow consistent style
