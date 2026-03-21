# Read/Write Ramfs - Future Enhancement

## Background

Ramfs filesystems are conventionally read/write (Linux's ramfs supports full
POSIX file operations). Our ramfs is currently read-only: it holds static
`&'static [u8]` slices that point directly into the cpio initramfs image in
memory. There is no allocation, copying, or freeing of file data.

Making ramfs writable requires distinguishing between borrowed static data
(from initramfs) and owned allocated data (from writes), so the filesystem
knows what it can and cannot free.

## Data Ownership Model

### Current State

Every `RamfsNode::File` holds a `&'static [u8]` pointing into the initramfs
cpio region. Nothing is allocated; nothing needs to be freed.

### Proposed State

File data must track its provenance:

```rust
enum FileData {
    /// Borrowed from initramfs cpio image -- must not be freed.
    Static(&'static [u8]),
    /// Dynamically allocated -- must be freed on overwrite or delete.
    Owned(Vec<u8>),
}

enum RamfsNode {
    File { data: FileData },
    Dir { children: BTreeMap<String, &'static RamfsInode> },
}
```

Rules:
- **Static data** originates from initramfs unpacking. It points into the
  cpio memory region and must never be freed or reallocated.
- **Owned data** is created by write operations. It is heap-allocated and
  must be freed when the file is deleted or its contents are replaced.
- **Overwriting a Static file** replaces it with Owned data. The static
  slice is simply forgotten (the cpio region remains valid).
- **Overwriting an Owned file** frees the old Vec and replaces it.

## Required Changes

### 1. Ramfs write support

Add write operations to `RamfsFileOps`:

- **write()** -- Append or overwrite file data. If the file currently holds
  `FileData::Static`, convert to `FileData::Owned(Vec<u8>)` (copying the
  static data if appending, or replacing it outright).
- **create()** -- Create a new empty file (Owned with empty Vec).
- **mkdir()** -- Create a new empty directory. (Already partially supported
  via `insert_dir`.)
- **unlink()** -- Remove a file entry from its parent directory. Free the
  data if Owned.
- **rmdir()** -- Remove an empty directory entry.
- **truncate()** -- Truncate or extend a file's data.

### 2. Inode interface extensions

The `Inode` trait (in `file.rs`) will need methods to support mutation:

- `create(name, mode)` -- create a child file/directory
- `unlink(name)` -- remove a child entry
- Or a more general approach where write-capable filesystems implement an
  extended trait.

### 3. Initramfs population via VFS

Once ramfs supports write operations, the initramfs unpacker should migrate
from calling `ramfs.insert_file()` / `ramfs.insert_dir()` directly to using
standard VFS interfaces:

```rust
// Current: direct ramfs API
ramfs.insert_dir(filename);
ramfs.insert_file(filename, &cpio[data_start..data_end]);

// Future: standard VFS operations
vfs::vfs_mkdir(filename);
let mut f = vfs::vfs_create(filename);
vfs::vfs_write(&mut f, &cpio[data_start..data_end]);
```

This decouples initramfs unpacking from the specific filesystem
implementation and validates that the VFS write path works end-to-end during
boot.

**Important caveat:** When populating from initramfs via VFS writes, the
ramfs should ideally detect that the source data is static and store it as
`FileData::Static` to avoid unnecessary copies. This could be done via:
- A VFS hint/flag indicating the source buffer is static.
- A special "write_static" path for boot-time population.
- Accepting the copy cost for simplicity, since initramfs is typically small.

### 4. Memory considerations

- The cpio initramfs memory region must remain valid for the lifetime of any
  `FileData::Static` references. If we eventually want to reclaim that
  memory, all Static references must first be converted to Owned (copied out).
- Directory entries currently use `&'static RamfsInode` (leaked Box). A
  writable ramfs will need a strategy for reclaiming inode memory on delete,
  likely replacing leaked references with a different ownership scheme
  (e.g., Arc or an inode table with indices).

## When to Implement

Consider implementing this when:
- A writable root filesystem is needed (e.g., `/tmp`, `/var/run`)
- Processes need to create or modify files
- We want to support `mount` overlaying writable storage onto the initramfs
