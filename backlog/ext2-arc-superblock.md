# ext2: Replace &'static Ext2SuperBlock with Arc<Ext2SuperBlock>

## Problem

`Ext2SuperBlock::new()` leaks the superblock (`Box::leak`) so that inodes can
hold `&'static Ext2SuperBlock` back-references. This prevents `FileSystem::mount()`
from returning `Box<dyn SuperBlock>`, which in turn forces `vfs.rs` to carry a
`StaticSb` adapter struct to wrap the leaked reference in the mount table.

## What needs to change

**`src/fs/ext2/superblock.rs`**
- `Ext2SuperBlock::new()` return type: `&'static Self` → `Arc<Self>`
- Stop leaking; construct with `Arc::new(...)` instead of `Box::leak(Box::new(...))`

**`src/fs/ext2/inode.rs`**
- `Ext2InodeData.sb: &'static Ext2SuperBlock` → `Arc<Ext2SuperBlock>`
- `read_inode(&'static self, ...)` → `read_inode(self: &Arc<Self>, ...)`
- `get_or_read_inode(&'static self, ...)` → same
- `Ext2InodeData { sb: self, ... }` → `sb: Arc::clone(self)`

**`src/fs/ext2/dir.rs`**
- `DirEntryIter.sb: &'static Ext2SuperBlock` → `Arc<Ext2SuperBlock>`

**`src/file.rs`**
- `Inode.sb: Option<&'static dyn SuperBlock>` → `Option<Arc<dyn SuperBlock>>`
- All inode constructors across ext2, ramfs, procfs updated accordingly

**`src/fs/ext2/mod.rs`**
- `Ext2FileSystem::mount()` return type: `&'static dyn SuperBlock` → `Box<dyn SuperBlock>`
- Body: `Ok(Box::new(Ext2SuperBlock::new(volume)?))`

## Payoff

- `StaticSb` in `vfs.rs` can be deleted
- `FileSystem::mount()` uniformly returns `Box<dyn SuperBlock>` across all drivers
- No heap leaks from ext2 mount

## Why it's non-trivial

`Inode.sb` is a field on the shared VFS `Inode` struct used by every filesystem.
Changing it from `&'static dyn SuperBlock` to `Option<Arc<dyn SuperBlock>>` touches
every inode constructor in ext2, ramfs, and procfs. The ext2 inode cache also holds
`Arc<Inode>` values that embed `Arc<Ext2SuperBlock>`, creating a reference cycle
(superblock → inode cache → inodes → superblock). The cycle needs to be broken,
likely with `Weak<Ext2SuperBlock>` in `Ext2InodeData`, which adds complexity to
the inode cache lookup path.
