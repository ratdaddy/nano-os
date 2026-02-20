# Filesystem Naming Conventions

This document describes the naming conventions for filesystem implementations in nano-os.

## Core Principle

Each filesystem has a **filesystem name** that forms the root of all related type names:
- `ext2` - Extended Filesystem 2 (note: no "fs" suffix in the name)
- `ramfs` - RAM Filesystem
- `procfs` - Process Filesystem

This filesystem name is used consistently across all related types.

## VFS Trait Implementations (In-Memory)

These are the types that implement VFS traits (`SuperBlock`, `Inode`, etc.). They represent the in-memory, working instances of filesystem structures.

**Pattern:** `{FilesystemName}{TraitName}`

Examples:
```rust
// ext2
pub struct Ext2SuperBlock { ... }    // implements SuperBlock
pub struct Ext2Inode { ... }         // implements Inode

// ramfs
pub struct RamfsSuperBlock { ... }   // implements SuperBlock
pub struct RamfsInode { ... }        // implements Inode

// procfs
pub struct ProcfsSuperBlock { ... }  // implements SuperBlock
pub struct ProcfsInode { ... }       // implements Inode
```

**Note:** Capitalization follows Rust naming conventions:
- `Ext2SuperBlock` (capital 'B' in SuperBlock)
- `Ext2Inode` (capital 'I' in Inode)

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

**Note:** Capitalization matches the VFS type name for consistency:
- `Ext2SuperBlockDisk` (capital 'B' in SuperBlock)
- `Ext2InodeDisk` (capital 'I' in Inode)

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
│  Ext2* Type     │  ← Proper in-memory representation
│ (VFS trait impl)│
└─────────────────┘
```

**Example:**
```rust
// Read raw bytes from disk
let mut buf = [0u8; 1024];
volume.read_blocks(2, &mut buf)?;

// Temporarily cast to Disk type to access fields
let disk_sb = unsafe { &*(buf.as_ptr() as *const Ext2SuperBlockDisk) };

// Parse into proper in-memory type
let sb = Ext2SuperBlock {
    inodes_count: disk_sb.s_inodes_count,
    blocks_count: disk_sb.s_blocks_count,
    block_size: 1024 << disk_sb.s_log_block_size,  // Calculated, not stored on disk
    // ... parse other fields
};
// disk_sb is dropped here - no longer needed
```

## Filesystem Type Registration

The type that implements the `FileSystem` trait for registration with VFS.

**Pattern:** `{FilesystemName}FileSystem` (struct) and `{FILESYSTEM_NAME}_FS` (static instance)

Examples:
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

## Complete Type Hierarchy

### ext2 (on-disk filesystem)

```
Ext2FileSystem              → implements FileSystem (registration)
  └─ mount() reads disk:
       ├─ Ext2SuperBlockDisk   (temporary, parsed immediately)
       ├─ Ext2GroupDescDisk    (temporary, parsed immediately)
       └─ creates:
            Ext2SuperBlock     → implements SuperBlock (in-memory)
              ├─ inodes_count, blocks_count, etc. (parsed values)
              └─ creates:
                   Ext2Inode   → implements Inode (in-memory)
                     └─ size, blocks, etc. (parsed from Ext2InodeDisk)
```

### ramfs (in-memory filesystem)

```
RamfsFileSystem             → implements FileSystem (registration)
  └─ mount() creates:
       RamfsSuperBlock      → implements SuperBlock (per-mount instance)
         └─ creates: RamfsInode → implements Inode (per-file instance)
```

### procfs (virtual filesystem)

```
ProcfsFileSystem            → implements FileSystem (registration)
  └─ mount() creates:
       ProcfsSuperBlock     → implements SuperBlock (per-mount instance)
         └─ creates: ProcfsInode → implements Inode (per-file instance)
```

## Rationale

### Why suffix on-disk structures with "Disk"?

1. **VFS types are used 90% of the time** - they should have clean, natural names
2. **On-disk structures are special** - they represent raw bytes and need explicit handling
3. **Clear distinction** - `Ext2Inode` (VFS) vs `Ext2InodeDisk` (raw bytes) is unambiguous
4. **Matches Linux pattern** - Linux uses `ext2_inode` (on-disk) vs `ext2_inode_info` (in-memory)
5. **Temporary nature** - `Disk` types are only used during parsing, not stored

### Why NOT embed Disk types in VFS types?

1. **Different lifetimes** - Disk types are temporary during I/O, VFS types are long-lived
2. **Data transformation** - Many fields need calculation (e.g., block_size = 1024 << log_block_size)
3. **Cleaner API** - VFS types have Rust-friendly fields, not C-style packed layouts
4. **Memory efficiency** - Don't store redundant on-disk layout in memory

### Why use filesystem name as prefix?

1. **Namespace isolation** - `Ext2Inode`, `RamfsInode`, `ProcfsInode` are clearly different types
2. **Grep-friendly** - easy to find all ext2-related types with `grep Ext2`
3. **Import clarity** - `use ext2::Ext2Inode` vs `use ramfs::RamfsInode` is self-documenting

### Why match trait names?

1. **Obvious relationship** - `Ext2SuperBlock implements SuperBlock` is clear
2. **Consistent pattern** - all filesystems follow the same naming convention
3. **Type hints** - the name tells you what traits are implemented

## Filesystem Name Reference

| Filesystem | Name String | Rust Prefix | Has On-Disk Format |
|------------|-------------|-------------|-------------------|
| Extended FS 2 | `"ext2"` | `Ext2` | Yes |
| RAM FS | `"ramfs"` | `Ramfs` | No |
| Process FS | `"procfs"` | `Procfs` | No |

**Note:** The name string (used in `fs_type()`) matches the filesystem name, even though the Rust prefix may capitalize differently (e.g., `ext2` → `Ext2`).

## Future Filesystems

When adding new filesystems, follow this pattern:

**With on-disk format (ext4, fat32, etc.):**
```rust
pub struct Ext4FileSystem;              // Registration
pub struct Ext4SuperBlock { ... }       // VFS SuperBlock impl (in-memory)
pub struct Ext4Inode { ... }            // VFS Inode impl (in-memory)

#[repr(C)]
pub struct Ext4SuperBlockDisk { ... }   // On-disk layout (temporary)
#[repr(C)]
pub struct Ext4InodeDisk { ... }        // On-disk layout (temporary)
```

**Virtual/in-memory only (tmpfs, devfs, etc.):**
```rust
pub struct TmpfsFileSystem;             // Registration
pub struct TmpfsSuperBlock { ... }      // VFS SuperBlock impl
pub struct TmpfsInode { ... }           // VFS Inode impl
// No Disk types needed
```

---

Last updated: 2026-02-18
