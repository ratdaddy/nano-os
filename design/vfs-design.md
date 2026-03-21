# VFS Design

This document describes the complete filesystem and VFS layer design for
nano-os as implemented. It covers the trait and struct hierarchy, ownership
model, per-filesystem strategies, and dispatch mechanics.

---

## Core abstractions

Four traits and two concrete structs form the entire VFS.

### Traits

**`FileSystem`** — registered at boot, one per driver

```rust
pub trait FileSystem: Send + Sync {
    fn name(&self) -> &'static str;
    fn requires_device(&self) -> bool;
    fn mount(&self) -> Result<&'static dyn SuperBlock, Error>;
}
```

**`SuperBlock`** — one per mounted filesystem instance

```rust
pub trait SuperBlock: Send + Sync {
    fn root_inode(&self) -> Arc<Inode>;
    fn fs_type(&self) -> &'static str;
}
```

**`InodeOps`** — static ops table for *inode-level* (directory) operations

```rust
pub trait InodeOps: Send + Sync {
    fn lookup(&self, inode: &Arc<Inode>, name: &str)
        -> Result<Arc<Inode>, Error> {
        Err(Error::NotADirectory)   // default: not a directory
    }
    // Future: mkdir, create, link, unlink, ...
}
```

**`FileOps`** — static ops table for *open-file* operations

```rust
pub trait FileOps: Send + Sync {
    fn open(&self, inode: Arc<Inode>) -> Result<File, Error> { ... }
    fn read(&self, file: &mut File, buf: &mut [u8]) -> Result<usize, Error> { ... }
    fn write(&self, file: &mut File, buf: &[u8]) -> Result<usize, Error> { ... }
    fn readdir(&self, file: &mut File) -> Result<Vec<DirEntry>, Error> { ... }
}
```

All `InodeOps` and `FileOps` implementations are `static` singletons
referenced by `&'static dyn` pointers stored in each `Inode`.

---

### Concrete structs

**`Inode`** — one per filesystem node, always reference-counted

```rust
pub struct Inode {
    pub ino:       u64,                          // user-visible inode number
    pub file_type: FileType,                     // RegularFile | Directory | CharDevice
    pub len:       usize,
    pub iops:      &'static dyn InodeOps,        // directory ops
    pub fops:      &'static dyn FileOps,         // open-file ops
    pub sb:        Option<&'static dyn SuperBlock>,
    pub rdev:      Option<(u32, u32)>,           // CharDevice: (major, minor)
    pub fs_data:   Box<dyn Any + Send + Sync>,   // filesystem-private payload
}
```

**`File`** — one per open file descriptor

```rust
pub struct File {
    pub inode:  Arc<Inode>,
    pub fops:   &'static dyn FileOps,
    pub offset: usize,
}
```

`File.fops` may differ from `inode.fops`. On chardev open, the chardev
registry installs the *device* ops rather than the inode's ops.

---

## `Arc<Inode>` ownership

All inodes are wrapped in `Arc<Inode>`. The VFS never holds raw references
to inodes — every path through `lookup`, `open`, and `readdir` produces an
`Arc`.

Ownership topology varies by filesystem:

### ramfs — permanent ownership tree

```
Ramfs (SuperBlock owner)
  └─ root: Arc<Inode>
       └─ children: UnsafeCell<BTreeMap<String, Arc<Inode>>>
            └─ grandchildren: ...
```

All nodes are live for the filesystem lifetime. `Arc` is used for API
uniformity; the reference count never reaches zero. Mutation of the child
map during `insert_file` / `get_or_create_dir` is safe because it happens
single-threadedly before any concurrent readers exist (mount-time init).
`UnsafeCell` is used instead of a raw pointer cast to satisfy the borrow
checker without UB.

### procfs — no inode caching

```
ProcfsSuperBlock
  └─ root: Arc<Inode>    ← permanent
       │
       ├─ lookup("version")  → new Arc<Inode> each time    (freed when caller drops)
       └─ fops.open(inode)   → new Arc<Inode> with content  (freed when File drops)
```

Each `lookup` call allocates a fresh inode. Each `open` call generates the
file content and attaches it as `ProcfsFileData(Box<[u8]>)` in `fs_data`
of a second fresh inode. There is no in-memory state worth preserving:
procfs content is synthetic and generated in microseconds.

### ext2 — weak inode cache (planned)

```
Ext2SuperBlock
  └─ cache: UnsafeCell<BTreeMap<u32, Weak<Inode>>>
```

`get_or_load_inode(num)` checks the cache first. If `Weak::upgrade()`
succeeds, the cached `Arc` is returned (zero I/O). On miss or dead `Weak`,
the inode is loaded from disk and inserted. Dead entries are reaped at miss
time to bound cache growth. There is no explicit eviction: inodes are freed
naturally when all `Arc` clones are dropped.

---

## Filesystem-private data (`fs_data`)

Each filesystem stores its private node state in
`Inode::fs_data: Box<dyn Any + Send + Sync>` and retrieves it with
`downcast_ref::<T>()`, which checks `TypeId` at runtime (O(1), no
scanning).

| Filesystem | `fs_data` type | Used for |
|------------|---------------|----------|
| ramfs | `RamfsNode::Dir { children: UnsafeCell<BTreeMap<...>> }` | directory children |
| ramfs | `RamfsNode::File { data: Vec<u8> }` | file contents |
| ramfs | `RamfsNode::CharDevice` (unit) | type tag; major/minor in `rdev` |
| procfs | `ProcfsNode::Dir` (unit) | root directory |
| procfs | `ProcfsNode::File { entry: &'static ProcEntry }` | lookup-time inode |
| procfs | `ProcfsFileData(Box<[u8]>)` | open-time snapshot |
| ext2 (planned) | `Ext2InodeData { num, sb_ref, ... }` | disk inode fields + back-ref |

The `fops` field on the `Inode` encodes node kind implicitly: a directory
inode holds a `&RAMFS_DIR_OPS` pointer, a file inode holds `&RAMFS_FILE_OPS`.
`downcast_ref` provides a second, independent safety check within the
filesystem.

---

## Inode numbering

`Inode::ino` is the user-visible inode number returned in `fstat` `st_ino`.
It must not be a kernel pointer (information leak).

| Filesystem | Strategy |
|------------|----------|
| ramfs | `AtomicU64` counter on `Ramfs`; root = 1, others sequential from 2 |
| procfs | root = 1; file inodes = `entry_index + 2` (stable, deterministic) |
| ext2 | on-disk inode number from the inode table |

`inode_id()` (`Arc::as_ptr() as usize`) is kept for *internal* VFS identity
(e.g., mount-crossing detection). It is never exposed to userspace.

---

## VFS dispatch

```
vfs_open(path):
    path_walk: for each component → inode.iops.lookup(parent, name)
    final inode → inode.fops.open(inode)
        → for CharDevice: chardev_open() installs device FileOps
    returns File

vfs_read(file):
    file.fops.read(file, buf)

vfs_write(file):
    file.fops.write(file, buf)

vfs_readdir(file):
    file.fops.readdir(file)
```

The `fops` stored in `File` is resolved at open time and does not change.
For normal files it equals `inode.fops`. For character devices it is the
driver's ops installed by `chrdev_open`.

---

## Naming conventions

```
{Name}FileSystem    — implements FileSystem (e.g. RamfsFileSystem, Ext2FileSystem)
{NAME}_FS           — static instance      (e.g. RAMFS_FS, EXT2_FS)
{Name}SuperBlock    — implements SuperBlock
{Name}InodeOps      — implements InodeOps  (static singleton)
{Name}FileOps / {Name}DirOps — implements FileOps (static singletons)
{Name}Node          — fs_data payload enum/struct (ramfs, procfs)
{Name}*Disk         — #[repr(C)] on-disk layout, temporary during I/O only
```

On-disk structs keep specification field names (`s_inodes_count`, `i_mode`)
for easy cross-referencing. In-memory types use clean Rust names
(`inodes_count`, `mode`).

See `notes/filesystem-naming.md` for detailed naming rationale and the
complete type hierarchy per filesystem.

---

## Split between InodeOps and FileOps

Mirrors the Linux `inode_operations` / `file_operations` split:

- **`InodeOps`** — acts on the inode in the directory tree (`lookup`).
  Directories implement it with a real `lookup`; files and devices use the
  default (error).
- **`FileOps`** — acts on an open file handle (`read`, `write`, `readdir`,
  `open`). Every inode has a `FileOps`; the particular one installed
  determines what operations are valid.

A directory inode has both: `InodeOps` for `lookup` when it is the *parent*
in a path walk, and `FileOps` for `readdir` when it is the *target* of
an `open`.
