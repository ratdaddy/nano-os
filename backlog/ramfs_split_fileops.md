# Split ramfs FileOps by inode type

## Current state

`RamfsFileOps` is a single impl that handles all node types (File, Dir,
CharDevice). Each method matches on `RamfsNode` and returns
`Err(InvalidInput)` for unsupported combinations (e.g., read on a
directory, readdir on a file).

## Proposed change

Split into separate ops structs assigned at inode creation time:

- `RamfsDirOps` — implements `open` and `readdir`
- `RamfsFileOps` — implements `open`, `read`, `seek`
- CharDevice inodes already go through the chardev subsystem via `rdev()`,
  so they don't need their own ops

Each inode's `file_ops()` returns the appropriate static reference based on
its `RamfsNode` variant. Unsupported operations fall through to the default
`FileOps` methods which already return `Err(InvalidInput)`.

## Benefits

- Eliminates match-on-node-type boilerplate inside FileOps methods
- Matches the Linux pattern where `i_fop` is set per inode type
- Consistent with how procfs already separates `ProcfsDirOps` and
  `ProcfsFileOps`

## Scope

Only `src/ramfs.rs` changes. The `Inode` trait, `File`, and VFS layer are
unaffected.
