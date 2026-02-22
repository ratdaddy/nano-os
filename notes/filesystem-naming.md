# Filesystem Naming Conventions

This document describes the naming conventions for filesystem implementations in nano-os.

## Core Principle

Each filesystem has a **filesystem name** that forms the root of all related type names:
- `ext2` - Extended Filesystem 2 (note: no "fs" suffix in the name)
- `ramfs` - RAM Filesystem
- `procfs` - Process Filesystem

This filesystem name is used consistently across all related types.

## In-Memory Types

There is no per-filesystem `Inode` type. The VFS provides a single concrete
`Inode` struct shared by all filesystems. Filesystem-specific in-memory types
are the superblock, the ops table implementations, and the private node data
stored in `Inode::fs_data`.

**Patterns:**

| Kind | Pattern | Example |
|------|---------|---------|
| FileSystem registration | `{Name}FileSystem` | `Ext2FileSystem` |
| SuperBlock | `{Name}SuperBlock` | `Ext2SuperBlock` |
| InodeOps impl | `{Name}InodeOps` | `RamfsInodeOps` |
| FileOps impl | `{Name}FileOps` / `{Name}DirOps` | `ProcfsFileOps`, `ProcfsDirOps` |
| fs_data payload | `{Name}Node` / `{Name}InodeData` | `RamfsNode`, `Ext2InodeData` |

```rust
// ext2
pub struct Ext2FileSystem;          // implements FileSystem
pub struct Ext2SuperBlock { ... }   // implements SuperBlock
struct Ext2InodeOps;                // implements InodeOps (static singleton)
struct Ext2FileOps;                 // implements FileOps (static singleton)
struct Ext2InodeData { ... }        // stored in Inode::fs_data

// ramfs
pub struct RamfsFileSystem;
pub struct RamfsSuperBlock { ... }
struct RamfsInodeOps;
struct RamfsFileOps;
struct RamfsDirOps;
enum RamfsNode { Dir { ... }, File { ... }, CharDevice }  // stored in Inode::fs_data

// procfs
pub struct ProcfsFileSystem;
pub struct ProcfsSuperBlock { ... }
struct ProcfsInodeOps;
struct ProcfsFileOps;
struct ProcfsDirOps;
enum ProcfsNode { Dir, File { entry: &'static ProcEntry } }  // stored in Inode::fs_data
```

**Note:** `InodeOps` and `FileOps` implementors are private (`struct`, not `pub struct`)
and held as `static` singletons within the filesystem module. The `Inode` struct
references them via `&'static dyn InodeOps` / `&'static dyn FileOps`.

### Field Naming in In-Memory Types

**Use clean, Rust-friendly names without specification prefixes.**

❌ **Don't use spec prefixes:**
```rust
pub struct Ext2SuperBlock {
    pub s_inodes_count: u32,      // NO - spec prefix in memory
    pub s_blocks_count: u32,
    pub s_log_block_size: u32,
}
```

✅ **Do use clean names:**
```rust
pub struct Ext2SuperBlock {
    pub inodes_count: u32,        // YES - clear, Rust-friendly
    pub blocks_count: u32,
    pub block_size: u32,          // pre-calculated: 1024 << log_block_size
}
```

**Rationale:**
- In-memory types are used throughout the codebase
- Specification prefixes (`s_`, `i_`, `bg_`, etc.) add noise without value
- Rust naming conventions prefer clear, concise names
- Type context already provides scope (`sb.blocks_count` vs `inode_data.size`)

## On-Disk Structures (Raw Byte Layout)

These represent the **exact byte layout** of structures as stored on disk. They use `#[repr(C)]` to match the on-disk format exactly. Only filesystems with persistent on-disk formats need these.

**Pattern:** `{FilesystemName}{StructureName}Disk`

**Important:** These types should ONLY be used for reading/writing raw bytes. Once data is read from disk, it should be parsed into proper in-memory types. In-memory types should NOT contain `Disk` types as fields.

Examples:
```rust
// ext2 - has on-disk format
#[repr(C)]
pub struct Ext2SuperBlockDisk { ... }   // 1024 bytes of superblock data
#[repr(C)]
pub struct Ext2InodeDisk { ... }        // 128 bytes of inode data
#[repr(C)]
pub struct Ext2GroupDescDisk { ... }    // 32 bytes of group descriptor

// ramfs - no on-disk structures (purely in-memory)
// procfs - no on-disk structures (virtual filesystem)
```

### Field Naming in On-Disk Types

**Keep specification prefixes to match external documentation.**

✅ **Do use spec field names:**
```rust
#[repr(C)]
pub struct Ext2SuperBlockDisk {
    pub s_inodes_count: u32,      // YES - matches spec exactly
    pub s_blocks_count: u32,
    pub s_log_block_size: u32,
    // ... exactly as specified
}

#[repr(C)]
pub struct Ext2InodeDisk {
    pub i_mode: u16,              // YES - matches spec exactly
    pub i_uid: u16,
    pub i_size: u32,
    // ... exactly as specified
}
```

**Rationale:**
- On-disk types exist only to match specification byte layout
- Keeping spec field names makes cross-referencing documentation easier
- These types are temporary (only during parsing)
- Consistency with external documentation prevents mistakes

## Data Flow: Disk to Memory

```
┌─────────────────┐
│   Disk Bytes    │
└────────┬────────┘
         │ read_blocks()
         ▼
┌─────────────────┐
│ Ext2*Disk Type  │  ← #[repr(C)] matches disk layout exactly
│ (temporary)     │
└────────┬────────┘
         │ parse/convert
         ▼
┌─────────────────┐
│  Arc<Inode>     │  ← VFS inode with Ext2InodeData in fs_data
│  (long-lived)   │
└─────────────────┘
```

**Example:**
```rust
// Read raw bytes from disk
let mut buf = [0u8; 1024];
volume.read_blocks(2, &mut buf)?;

// Temporarily cast to Disk type to access fields
let disk_sb = unsafe { &*(buf.as_ptr() as *const Ext2SuperBlockDisk) };

// Parse into proper in-memory superblock
let sb = Ext2SuperBlock {
    inodes_count: disk_sb.s_inodes_count,
    blocks_count: disk_sb.s_blocks_count,
    block_size: 1024 << disk_sb.s_log_block_size,  // calculated, not stored on disk
    // ...
};
// disk_sb is dropped here
```

## Filesystem Type Registration

The type that implements the `FileSystem` trait for registration with VFS.

**Pattern:** `{FilesystemName}FileSystem` (struct) and `{FILESYSTEM_NAME}_FS` (static instance)

```rust
// ext2
pub struct Ext2FileSystem;           // implements FileSystem trait
pub static EXT2_FS: Ext2FileSystem = Ext2FileSystem;

// ramfs
pub struct RamfsFileSystem;
pub static RAMFS_FS: RamfsFileSystem = RamfsFileSystem;

// Registration
vfs::register_filesystem(&EXT2_FS);
vfs::register_filesystem(&RAMFS_FS);
```

The `FileSystem` type and its static instance are placed at the **top** of the
source file, before internal implementation types.

## Complete Type Hierarchy

### ext2 (on-disk filesystem)

```
Ext2FileSystem                  implements FileSystem (registration)
  └─ mount() reads disk:
       ├─ Ext2SuperBlockDisk        (temporary, parsed immediately)
       ├─ Ext2GroupDescDisk[]       (temporary, parsed immediately)
       └─ creates Ext2SuperBlock    implements SuperBlock
            └─ root_inode() loads from disk → Arc<Inode>
                 ├─ iops: &Ext2InodeOps
                 ├─ fops: &Ext2FileOps or &Ext2DirOps
                 └─ fs_data: Ext2InodeData { num, size, blocks, ... }
                      └─ populated by reading Ext2InodeDisk (temporary)
```

### ramfs (in-memory filesystem)

```
RamfsFileSystem                 implements FileSystem (registration)
  └─ mount() creates Ramfs → RamfsSuperBlock    implements SuperBlock
       └─ root: Arc<Inode>
            ├─ iops: &RamfsInodeOps
            ├─ fops: &RamfsDirOps
            └─ fs_data: RamfsNode::Dir { children: UnsafeCell<BTreeMap<...>> }
                 └─ each child: Arc<Inode>
                      └─ fs_data: RamfsNode::File { data } | RamfsNode::CharDevice
```

### procfs (virtual filesystem)

```
ProcfsFileSystem                implements FileSystem (registration)
  └─ mount() creates ProcfsSuperBlock    implements SuperBlock
       └─ root: Arc<Inode>   (permanent)
            └─ lookup(name) → new Arc<Inode>   (per-lookup, not cached)
                 └─ fops.open() → new Arc<Inode> with ProcfsFileData   (per-open)
```

## Adding a New Filesystem

**With on-disk format (e.g., fat32):**
```rust
pub struct Fat32FileSystem;              // Registration
pub static FAT32_FS: Fat32FileSystem = Fat32FileSystem;

pub struct Fat32SuperBlock { ... }       // implements SuperBlock; holds parsed metadata
struct Fat32InodeOps;                    // implements InodeOps  (static singleton)
struct Fat32FileOps;                     // implements FileOps   (static singleton)
struct Fat32DirOps;                      // implements FileOps   (static singleton)
struct Fat32InodeData { ... }            // stored in Inode::fs_data

#[repr(C)] struct Fat32BootSectorDisk { ... }   // on-disk layout (temporary)
#[repr(C)] struct Fat32DirEntryDisk { ... }      // on-disk layout (temporary)
```

**Virtual/in-memory only (e.g., devfs):**
```rust
pub struct DevfsFileSystem;
pub static DEVFS_FS: DevfsFileSystem = DevfsFileSystem;

pub struct DevfsSuperBlock { ... }       // implements SuperBlock
struct DevfsInodeOps;                    // implements InodeOps
struct DevfsDirOps;                      // implements FileOps
enum DevfsNode { ... }                   // stored in Inode::fs_data
// No Disk types needed
```

## Rationale

### Why suffix on-disk structures with "Disk"?

1. **VFS inodes are shared** — there is no per-filesystem inode type to
   disambiguate from; the `Disk` suffix marks raw-byte-layout structs clearly
2. **Temporary nature** — `Disk` types live only during a single I/O parse
3. **Clear distinction** — `Ext2InodeDisk` (128 raw bytes) vs `Ext2InodeData`
   (parsed fields in `fs_data`) is unambiguous
4. **Matches Linux pattern** — Linux uses `ext2_inode` (on-disk) vs
   `ext2_inode_info` (in-memory)

### Why NOT embed Disk types in VFS types?

1. **Different lifetimes** — Disk types are temporary during I/O, in-memory
   types are long-lived
2. **Data transformation** — many fields need calculation
   (e.g., `block_size = 1024 << log_block_size`)
3. **Cleaner API** — in-memory types have Rust-friendly fields, not packed
   C-style layouts
4. **Memory efficiency** — don't store redundant on-disk layout in memory

### Why use filesystem name as prefix?

1. **Namespace isolation** — `Ext2InodeOps`, `RamfsInodeOps`, `ProcfsInodeOps`
   are clearly different types
2. **Grep-friendly** — easy to find all ext2-related types with `grep Ext2`
3. **Consistent with SuperBlock** — `Ext2SuperBlock implements SuperBlock`
   pattern extends naturally to ops types

## Filesystem Name Reference

| Filesystem | Name String | Rust Prefix | Has On-Disk Format |
|------------|-------------|-------------|-------------------|
| Extended FS 2 | `"ext2"` | `Ext2` | Yes |
| RAM FS | `"ramfs"` | `Ramfs` | No |
| Process FS | `"procfs"` | `Procfs` | No |

**Note:** The name string (used in `fs_type()`) matches the filesystem name,
even though the Rust prefix may capitalize differently (e.g., `ext2` → `Ext2`).

---

Last updated: 2026-02-21
