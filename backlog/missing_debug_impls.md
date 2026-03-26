# Missing Debug impls on VFS and filesystem types

## Problem

Several types in the VFS and filesystem layer are missing `#[derive(Debug)]`,
which is required by `ref/rust-trait-checklist.md`. This makes them invisible
in log output, panic messages, and test failures.

## Blocked types (require `Inode: Debug` first)

- `RamfsNode` — `Dir` variant contains `Arc<Inode>`
- `RamfsSuperBlock` — contains `Arc<Inode>`
- `Ramfs` — contains `Arc<Inode>`

These are blocked because `Inode` in `src/file.rs` contains:

```rust
pub fs_data: Box<dyn Any + Send + Sync>,
```

`dyn Any` does not implement `Debug`, so `#[derive(Debug)]` on `Inode` won't
compile. Options:

1. Widen the bound: `Box<dyn Any + Send + Sync + Debug>` — requires all
   `fs_data` values to implement `Debug` (ramfs nodes, ext2 data, etc.)
2. Manual `Debug` impl on `Inode` that omits or summarizes `fs_data`

Option 1 is cleaner but requires adding `Debug` to every `fs_data` type first.
Option 2 is a reasonable stopgap.

## Inspection validation

Once `Inode: Debug` and `RamfsNode: Debug` are in place, add a boot menu
inspect option that dumps the ramfs tree structurally:

```rust
println!("{:#?}", ramfs);
```

This would show the full directory tree with node types, inode numbers, and
file data slices — more useful for debugging mount/population issues than the
current `7) Filesystem contents` option, which only shows what VFS can read
back out.

## Other filesystem files to check

Once `Inode: Debug` is resolved, apply the same sweep to `procfs.rs` and
`src/fs/ext2/`.

## Discovered during

Pre-commit check for `refcell-ramfs` — trait checklist applied to all types
in `src/fs/ramfs.rs`.
