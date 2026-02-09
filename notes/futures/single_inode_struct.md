# Single inode struct (Linux-style)

## Current state

`Inode` is a trait. Each filesystem defines its own inode types that
implement it (e.g., `RamfsInode`, `ProcfsDirInode`, `ProcfsFileInode`).
The VFS layer works with `&'static dyn Inode` trait objects throughout.

Inode-level operations (`lookup`) live on the `Inode` trait. File-level
operations (`read`, `write`, `seek`, `readdir`, `open`) live on the
`FileOps` trait.

## Proposed change

Replace the `Inode` trait with a concrete struct, matching Linux's single
`struct inode`:

```rust
pub trait InodeOps: Send + Sync {
    fn lookup(&self, inode: &'static Inode, name: &str)
        -> Result<&'static Inode, Error> {
        Err(Error::InvalidInput)
    }
    // Future: mkdir, create, link, unlink, symlink, ...
}

pub struct Inode {
    pub file_type: FileType,
    pub len: usize,
    pub iops: &'static dyn InodeOps,
    pub fops: &'static dyn FileOps,
    pub sb: Option<&'static dyn SuperBlock>,
    pub rdev: Option<(u32, u32)>,
    pub fs_data: &'static dyn Any,
}
```

Each filesystem constructs `Inode` structs directly rather than
implementing a trait. Filesystem-specific data (ramfs node enum, procfs
entry reference) is stored in `fs_data` and accessed via
`downcast_ref()`.

## Separation of ops tables

This mirrors the Linux split between `inode_operations` and
`file_operations`:

- **InodeOps** — operations on the inode itself: `lookup`, and
  eventually `mkdir`, `create`, `unlink`, etc.
- **FileOps** — operations on an open file handle: `read`, `write`,
  `seek`, `readdir`, `open`, `close`

Directories get an `InodeOps` with `lookup` implemented. Files and
devices get one with the default error return. Each filesystem provides
static `InodeOps` and `FileOps` tables assigned at inode creation time.

## Benefits

- Basic inode fields (`file_type`, `len`, `sb`) are direct struct reads
  — no vtable dispatch
- `&'static Inode` replaces `&'static dyn Inode` — simpler, no fat
  pointers
- `inode_id()` becomes trivial (thin pointer address)
- No per-filesystem Inode trait boilerplate
- Matches Linux architecture closely

## Tradeoffs

- `fs_data` still requires `downcast_ref()` for filesystem-specific
  access, so runtime-checked dispatch doesn't fully go away
- Touches nearly every file: `File`, `vfs.rs`, `ramfs.rs`, `procfs.rs`,
  `chardev.rs`, `initramfs.rs`, all tests, all demos
- `InodeOps` methods need an explicit `&'static Inode` parameter since
  they're no longer trait methods on the inode itself

## Files affected

| File | Change |
|------|--------|
| `src/file.rs` | Replace `Inode` trait with struct, add `InodeOps` trait |
| `src/vfs.rs` | `&'static dyn Inode` → `&'static Inode`, use `iops` for lookup |
| `src/ramfs.rs` | Remove `impl Inode for RamfsInode`, construct `Inode` structs |
| `src/procfs.rs` | Same as ramfs |
| `src/chardev.rs` | Update inode references |
| `src/initramfs.rs` | Update inode references |
| `src/kprint.rs` | Update `File` usage if `File.inode` type changes |
| All tests and demos | Update to new types |
