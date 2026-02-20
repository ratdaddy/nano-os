# ext2 On-Disk Format Specification

Authoritative sources:
- https://www.nongnu.org/ext2-doc/ext2.html
- https://wiki.osdev.org/Ext2
- https://docs.kernel.org/filesystems/ext2.html

## Layout Overview

```
Boot Block (1024 bytes) | Superblock (1024 bytes) | Block Group Descriptor Table | Block Groups...
```

- **Boot block**: Bytes 0-1023 (reserved for boot code)
- **Superblock**: Always at byte offset 1024, size 1024 bytes
- **Block Groups**: Filesystem divided into equal-sized groups

## Superblock Structure

**Location**: Byte offset 1024 (sector 2 for 512-byte sectors)
**Size**: 1024 bytes
**Magic**: 0xEF53 at offset 56

### Core Fields (All Revisions)

| Offset | Size | Type | Field | Description |
|--------|------|------|-------|-------------|
| 0 | 4 | u32 | s_inodes_count | Total inodes in filesystem |
| 4 | 4 | u32 | s_blocks_count | Total blocks in filesystem |
| 8 | 4 | u32 | s_r_blocks_count | Reserved blocks for superuser |
| 12 | 4 | u32 | s_free_blocks_count | Free blocks count |
| 16 | 4 | u32 | s_free_inodes_count | Free inodes count |
| 20 | 4 | u32 | s_first_data_block | First data block (0 or 1) |
| 24 | 4 | u32 | s_log_block_size | Block size = 1024 << value |
| 28 | 4 | u32 | s_log_frag_size | Fragment size (unused) |
| 32 | 4 | u32 | s_blocks_per_group | Blocks per block group |
| 36 | 4 | u32 | s_frags_per_group | Fragments per group (unused) |
| 40 | 4 | u32 | s_inodes_per_group | Inodes per block group |
| 44 | 4 | u32 | s_mtime | Last mount time (Unix timestamp) |
| 48 | 4 | u32 | s_wtime | Last write time (Unix timestamp) |
| 52 | 2 | u16 | s_mnt_count | Mounts since last fsck |
| 54 | 2 | u16 | s_max_mnt_count | Max mounts before fsck |
| 56 | 2 | u16 | s_magic | Magic signature (0xEF53) |
| 58 | 2 | u16 | s_state | Filesystem state |
| 60 | 2 | u16 | s_errors | Error handling behavior |
| 62 | 2 | u16 | s_minor_rev_level | Minor revision level |
| 64 | 4 | u32 | s_lastcheck | Last check time |
| 68 | 4 | u32 | s_checkinterval | Max time between checks |
| 72 | 4 | u32 | s_creator_os | OS that created filesystem |
| 76 | 4 | u32 | s_rev_level | Revision (0=old, 1=dynamic) |
| 80 | 2 | u16 | s_def_resuid | Default reserved UID |
| 82 | 2 | u16 | s_def_resgid | Default reserved GID |

### Extended Fields (Revision 1+ only)

| Offset | Size | Type | Field | Description |
|--------|------|------|-------|-------------|
| 84 | 4 | u32 | s_first_ino | First usable inode (11 if rev 0) |
| 88 | 2 | u16 | s_inode_size | Inode size (128 if rev 0) |
| 90 | 2 | u16 | s_block_group_nr | Superblock's block group |
| 92 | 4 | u32 | s_feature_compat | Compatible features |
| 96 | 4 | u32 | s_feature_incompat | Incompatible features |
| 100 | 4 | u32 | s_feature_ro_compat | Read-only compatible features |
| 104 | 16 | u8[16] | s_uuid | Volume UUID |
| 120 | 16 | char[16] | s_volume_name | Volume label (null-terminated) |
| 136 | 64 | char[64] | s_last_mounted | Last mount path |

**Important**:
- If `s_rev_level == 0`: inode size is always 128 bytes, first usable inode is 11
- If `s_rev_level >= 1`: read `s_inode_size` from offset 88

## Block Group Descriptor Table

**Location**: Immediately after superblock
- 1KB blocks: Block 2 (byte 2048)
- 2KB+ blocks: Block 1 (byte block_size)

**Entry Size**: 32 bytes
**Count**: `(s_blocks_count + s_blocks_per_group - 1) / s_blocks_per_group`

### Group Descriptor Structure

| Offset | Size | Type | Field | Description |
|--------|------|------|-------|-------------|
| 0 | 4 | u32 | bg_block_bitmap | Block number of block bitmap |
| 4 | 4 | u32 | bg_inode_bitmap | Block number of inode bitmap |
| 8 | 4 | u32 | bg_inode_table | Block number of inode table |
| 12 | 2 | u16 | bg_free_blocks_count | Free blocks in group |
| 14 | 2 | u16 | bg_free_inodes_count | Free inodes in group |
| 16 | 2 | u16 | bg_used_dirs_count | Directory count |
| 18 | 2 | u16 | bg_pad | Padding |
| 20 | 12 | u8[12] | bg_reserved | Reserved |

**Note**: All block numbers are absolute filesystem block numbers, not group-relative.

## Inode Structure

**Size**: Read from `s_inode_size` in superblock (or 128 if revision 0)
**Location**: In inode table at block `bg_inode_table` for the group

### Inode Lookup Formula

```
group = (inode_number - 1) / s_inodes_per_group
index = (inode_number - 1) % s_inodes_per_group
block = bg_inode_table[group] + (index * s_inode_size) / block_size
offset_in_block = (index * s_inode_size) % block_size
```

### Inode Fields (First 128 bytes)

| Offset | Size | Type | Field | Description |
|--------|------|------|-------|-------------|
| 0 | 2 | u16 | i_mode | File type and permissions |
| 2 | 2 | u16 | i_uid | Owner UID (lower 16 bits) |
| 4 | 4 | u32 | i_size | File size in bytes (lower 32 bits) |
| 8 | 4 | u32 | i_atime | Last access time |
| 12 | 4 | u32 | i_ctime | Creation/status change time |
| 16 | 4 | u32 | i_mtime | Last modification time |
| 20 | 4 | u32 | i_dtime | Deletion time |
| 24 | 2 | u16 | i_gid | Owner GID (lower 16 bits) |
| 26 | 2 | u16 | i_links_count | Hard link count |
| 28 | 4 | u32 | i_blocks | 512-byte blocks allocated |
| 32 | 4 | u32 | i_flags | File flags |
| 36 | 4 | u32 | i_osd1 | OS-dependent value |
| 40 | 60 | u32[15] | i_block | Block pointers array |
| 100 | 4 | u32 | i_generation | File version (NFS) |
| 104 | 4 | u32 | i_file_acl | Extended attributes block |
| 108 | 4 | u32 | i_dir_acl | Directory ACL / size high bits |
| 112 | 4 | u32 | i_faddr | Fragment address (obsolete) |
| 116 | 12 | u8[12] | i_osd2 | OS-dependent structure |

### File Type in i_mode

```
i_mode & 0xF000:
  0x1000 = FIFO
  0x2000 = Character device
  0x4000 = Directory
  0x6000 = Block device
  0x8000 = Regular file
  0xA000 = Symbolic link
  0xC000 = Socket
```

### Reserved Inode Numbers

- 0: No inode (null)
- 1: Bad blocks inode
- 2: Root directory (always)
- 3: ACL index
- 4: ACL data
- 5: Boot loader
- 6: Undelete directory
- 7: Reserved group descriptors
- 8: Journal inode (ext3)
- 9: Exclude inode
- 10: Replica inode
- 11+: Regular files/directories

### Block Pointers (i_block array)

- **i_block[0..11]**: Direct block pointers (12 blocks)
- **i_block[12]**: Single indirect block (points to block of pointers)
- **i_block[13]**: Double indirect block
- **i_block[14]**: Triple indirect block

**Special case for device files**:
- For character and block devices, `i_block[0]` contains the device number
- Format: `(major << 8) | minor`
- Example: `/dev/console` (major=5, minor=1) stores `0x0501` in `i_block[0]`

## Directory Entry Structure

Directory files contain a sequence of variable-length entries.

| Offset | Size | Type | Field | Description |
|--------|------|------|-------|-------------|
| 0 | 4 | u32 | inode | Inode number (0 = unused entry) |
| 4 | 2 | u16 | rec_len | Bytes to next entry |
| 6 | 1 | u8 | name_len | Name length (0-255) |
| 7 | 1 | u8 | file_type | Type indicator |
| 8+ | variable | char[] | name | File name (not null-terminated) |

### File Types (file_type field)

- 0: Unknown
- 1: Regular file
- 2: Directory
- 3: Character device
- 4: Block device
- 5: FIFO
- 6: Socket
- 7: Symbolic link

### Directory Entry Notes

- `rec_len` includes the entire entry (header + name + padding)
- Entries are 4-byte aligned
- Last entry in a block has `rec_len` extending to block end
- To iterate: `next_entry = current_entry + rec_len`
- Stop when `rec_len == 0` or beyond directory size

## Common Pitfalls

1. **Inode size**: MUST read from superblock (offset 88), don't assume 128 bytes
2. **Block size**: Calculate as `1024 << s_log_block_size`, don't hardcode
3. **Inode numbering**: 1-indexed, subtract 1 for array index
4. **Root inode**: Always inode #2 (not read from anywhere)
5. **Group descriptor location**: Depends on block size (block 1 or 2)
6. **All integers**: Little-endian byte order
7. **Directory names**: NOT null-terminated, use `name_len`
