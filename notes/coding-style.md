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

## Dead Code and Test Builds

**Principle:** Suppress dead code warnings globally during tests via a crate-level attribute. Do not gate modules or items with `#[cfg(not(test))]` or add per-item `#[cfg_attr(test, allow(dead_code))]`.

The crate root (`main.rs`) has:
```rust
#![cfg_attr(test, allow(dead_code))]
```

This means:
- All modules are always compiled (rust-analyzer sees everything — neovim works correctly)
- Dead code warnings are suppressed globally during `make test`
- **New modules need no special treatment** — the crate-level attribute covers them automatically

Use plain `#[allow(dead_code)]` only when an item should always be allowed to be unused regardless of build mode (e.g., a struct field kept for documentation or future use).

**Don't do this:**
```rust
#[cfg(not(test))]        // Hides module from rust-analyzer in test mode
mod my_module;

#[cfg_attr(test, allow(dead_code))]  // Per-item suppression is now redundant
pub fn my_fn() { ... }
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

## Numeric Literal Formatting

**Principle:** Use lowercase hex digits and octal for POSIX/Unix values.

### Hex literals

Always use lowercase `a–f`, matching Rust idiom and the standard library:

```rust
// Bad
const MBR_SIGNATURE: u16 = 0xaa55;
const BLOCK_SIZE: usize = 0x200;

// Good
const MBR_SIGNATURE: u16 = 0xaa55;
const BLOCK_SIZE: usize = 0x200;
```

### Octal literals

Use octal for POSIX mode bits and Unix permission values — these are universally
documented and discussed in octal, so octal literals are the most readable form:

```rust
// Bad - requires mental conversion to recognise as POSIX mode bits
pub const S_IFREG: u16 = 0x8000;

// Good - immediately recognisable as a POSIX file type constant
pub const S_IFREG: u16 = 0o100000;
```

---

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
const MBR_SIGNATURE: u16 = 0xaa55;
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
const MBR_SIGNATURE: u16 = 0xaa55;
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
const MBR_SIGNATURE: u16 = 0xaa55;
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

## Smart Pointer Dereferencing

**Principle:** Use explicit methods over implicit dereferencing operators.

When converting smart pointers to references, prefer explicit methods like `as_ref()` over the `&*` pattern for clarity.

```rust
// Bad - implicit dereference then reference
let volume_ref: &dyn BlockVolume = &*arc_volume;
function_call(&*arc_volume);

// Good - explicit conversion
let volume_ref: &dyn BlockVolume = arc_volume.as_ref();
function_call(arc_volume.as_ref());
```

**Exception:** Raw pointer dereferencing (`&*ptr` from `*const T` or `*mut T`) should remain as-is since there's no `as_ref()` alternative.

## Struct Initialization from External Data

**Principle:** When initializing a struct from external data (disk, network, parsing), prefer the mutable builder pattern over large tuple returns.

### Pattern: Mutable Builder

When a constructor needs to populate many fields from external sources, initialize with placeholder values then populate with methods:

```rust
// Good - mutable builder pattern
impl MyStruct {
    pub fn new(source: DataSource) -> Result<Self, Error> {
        // Initialize with placeholder values
        let mut obj = Self {
            source,
            field1: 0,
            field2: 0,
            field3: None,
            cached_data: Vec::new(),
        };

        // Populate fields from external data
        obj.read_metadata()?;
        obj.read_cached_data()?;

        Ok(obj)
    }

    fn read_metadata(&mut self) -> Result<(), Error> {
        // Read from self.source, populate self.field1, self.field2, etc.
        self.field1 = parse_field1(&data);
        self.field2 = parse_field2(&data);
        Ok(())
    }

    fn read_cached_data(&mut self) -> Result<(), Error> {
        // Populate self.cached_data
        Ok(())
    }
}

// Bad - large tuple destructuring
impl MyStruct {
    pub fn new(source: DataSource) -> Result<Self, Error> {
        let (field1, field2, field3) = Self::read_metadata(&source)?;
        let cached_data = Self::read_cached_data(&source, field1, field2)?;

        Ok(Self {
            source,
            field1,
            field2,
            field3,
            cached_data,
        })
    }

    fn read_metadata(source: &DataSource) -> Result<(u32, u32, Option<String>), Error> {
        // Awkward tuple return
        Ok((val1, val2, val3))
    }
}
```

### Advantages

- **No tuple destructuring** - cleaner code, easier to read
- **Direct field assignment** - clear what's being set where
- **Methods can call other methods** - use `self.num_groups()`, `self.block_size()` instead of passing parameters
- **More extensible** - adding fields doesn't change signatures

### Temporary Invalid State

The struct is briefly in an invalid state (zero/empty values) during construction. This is acceptable because:
- State is **private** to the constructor - never observable externally
- Common Rust pattern (e.g., `Vec::with_capacity`, `Default` trait)
- Constructor either succeeds (fully valid) or fails (no object created)

## Function Organization

**Principle:** Helper functions related to a type should be associated functions in that type's impl block.

### When to use associated functions

If a function:
- Is only called from methods of a specific type
- Doesn't need `&self` (stateless helper)
- Logically belongs to that type's implementation

Then it should be an **associated function** in the type's impl block.

### Pattern

```rust
// Bad - standalone private function
fn read_data(source: &DataSource) -> Result<Data, Error> {
    // implementation
}

impl MyType {
    pub fn new(source: DataSource) -> Result<Self, Error> {
        let data = read_data(&source)?;  // Unclear where read_data comes from
        Ok(Self { data })
    }
}

// Good - associated function
impl MyType {
    pub fn new(source: DataSource) -> Result<Self, Error> {
        let data = Self::read_data(&source)?;  // Clear this is a MyType function
        Ok(Self { data })
    }

    fn read_data(source: &DataSource) -> Result<Data, Error> {
        // implementation
    }
}
```

### When standalone functions are appropriate

Keep functions as module-level standalone when:
- Used by multiple unrelated types in the module
- Generic utilities not tied to any specific type
- Required to be standalone (e.g., function pointers in arrays)
- Part of module's public API but not type-specific

```rust
// Appropriate standalone - used as function pointer
fn gen_data() -> String { /* ... */ }
static GENERATORS: [fn() -> String; 2] = [gen_data, gen_other];

// Appropriate standalone - generic utility
fn align_up(x: usize, align: usize) -> usize {
    (x + align - 1) & !(align - 1)
}
```

## Tuples vs Structs

**Principle:** Prefer named structs over tuples when a type has more than one field or when fields are discarded at call sites.

Tuples are appropriate for truly ad-hoc, short-lived groupings where the positional meaning is obvious. As soon as callers start writing `_` to discard fields, or need to remember which position holds which value, a named struct is clearer.

```rust
// Bad - caller must track positions, discards with _
type Item = Result<(u32, String, u8), Error>;

for entry in iter {
    let (ino, name, _) = entry?;   // what was the third field again?
}

// Good - fields are self-documenting
struct RawDirEntry {
    pub ino: u32,
    pub name: String,
    pub file_type: u8,
}

for entry in iter {
    let entry = entry?;
    if entry.name == target { ... entry.ino ... }
}
```

Tuples are fine for:
- Returning two tightly related values where order is obvious (`(start, end)`, `(major, minor)`)
- Short-lived internal helpers not exposed beyond a few lines

## Code Review Checklist

Before committing:

- [ ] All imports follow the external → blank → internal pattern
- [ ] No `crate::` or `core::` or `alloc::` usage at call sites (only in `use` declarations)
- [ ] Mutable statics use `&raw mut` pattern
- [ ] No unnecessary nested `unsafe` blocks
- [ ] Method names clearly indicate async vs sync behavior
- [ ] Magic numbers replaced with appropriately named constants
- [ ] Hex literals use lowercase digits (`0xaa55` not `0xAA55`); POSIX mode bits use octal
- [ ] Smart pointer conversions use `as_ref()` not `&*`
- [ ] Helper functions are associated functions when appropriate
- [ ] Struct initialization uses mutable builder pattern instead of large tuple returns
- [ ] Multi-field return types use named structs, not tuples (especially if any field is discarded with `_` at call sites)
- [ ] All modified files follow consistent style
