# ext2 lookup maps disk errors to NotFound

## Location

`src/fs/ext2/ops.rs` — `Ext2InodeOps::lookup`

## Current code

```rust
for entry in DirEntryIter::new(inode) {
    let (ino, entry_name, _) = entry.map_err(|_| Error::NotFound)?;
    ...
}
```

## Problem

A disk I/O error from `DirEntryIter` is silently converted to `Error::NotFound`.
The caller cannot distinguish "the entry does not exist" from "the disk read failed".
`readdir` in the same file correctly maps disk errors to `Error::InvalidInput`, which
at least preserves the distinction.

## Fix

Map the `BlockError` to a more appropriate VFS error — either `Error::InvalidInput`
(consistent with `readdir`) or a dedicated `Error::Io` variant if one is added:

```rust
let (ino, entry_name, _) = entry.map_err(|_| Error::InvalidInput)?;
```

A proper `Error::Io` variant would allow callers to distinguish transient hardware
failures from logical filesystem errors, which matters once write support is added.
