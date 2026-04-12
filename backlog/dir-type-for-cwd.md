# Introduce a Dir type for directory-typed inodes

## Context

Currently `Arc<Inode>` is used everywhere regardless of file type. Code that
requires a directory inode validates it at runtime with a `file_type` check
and returns `Err(NotADirectory)` on mismatch. This check is repeated in
`Ext2InodeOps::lookup`, `Ext2FileOps::readdir`, the mock filesystem in VFS
tests, and will appear in each future syscall that requires a directory fd.

## Proposed change

Introduce a `Dir` newtype wrapping `Arc<Inode>` with a single construction
point that enforces the directory invariant:

```rust
pub struct Dir(Arc<Inode>);

impl Dir {
    pub fn try_from(inode: Arc<Inode>) -> Result<Self, Error> {
        if inode.file_type != FileType::Directory {
            return Err(Error::NotADirectory);
        }
        Ok(Self(inode))
    }
}
```

`IntoIterator` on `Dir` would yield `DirEntry` naturally, removing ambiguity
about what iterating an `Inode` means.

## Primary motivation: working directory

The strongest argument for this type is the process working directory. When
`chdir`/`fchdir` are implemented, cwd must always be a directory. Storing it
as `Arc<Inode>` requires discipline and comments to maintain that invariant.
Storing it as `Dir` makes the invariant structural — `chdir` becomes a `Dir`
constructor and the per-process cwd field cannot hold a non-directory.

## Secondary benefit: `*at` syscall family

`openat`, `mkdirat`, `fchdir`, `getdents`, and `renameat` all require a
directory-typed fd. Each currently (or will) do a runtime check at syscall
entry. A helper that converts fd → `Dir` would centralize this pattern. The
repetition is moderate — around 5–6 syscalls — so this is a secondary concern.

## When to act

Implement alongside or just before `chdir`/`fchdir`. Not worth doing in
isolation; the payoff comes when cwd storage is introduced.

## Scope

- New `Dir` type in `src/file.rs`
- `IntoIterator for Dir` delegating to `DirEntryIter` (ext2) or equivalent
- Per-process cwd field typed as `Option<Dir>`
- Syscall entry helpers for fd → `Dir` conversion
- Existing `file_type` checks in `InodeOps`/`FileOps` impls can be removed
  where `Dir` is passed instead of `Arc<Inode>`
