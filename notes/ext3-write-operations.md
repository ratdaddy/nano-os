# ext3 Write Operations

Architectural reference for implementing write support. Covers every VFS mutation,
the on-disk objects each one touches, and how ext3 journaling (data=ordered mode)
treats them.

This document will grow into the implementation guide as write support is added.

---

## Metadata vs. File Data (data=ordered mode)

ext3's `data=ordered` mode (the default, and what we will implement) divides writes
into two categories:

**Metadata — always journaled:**
- Superblock (`s_free_blocks_count`, `s_free_inodes_count`)
- Group descriptors (`bg_free_blocks_count`, `bg_free_inodes_count`, `bg_used_dirs_count`)
- Inode bitmap blocks
- Block bitmap blocks
- Inode table blocks (all inode fields: mode, size, timestamps, block pointers, links_count)
- Directory data blocks (directory entry content is metadata, not file data)
- Indirect pointer blocks (single, double, triple — pointer blocks, not the data they point to)

**File data — not journaled, written directly to final location:**
- Regular file data blocks only

The ordering guarantee of `data=ordered`: all file data writes must complete before the
journal transaction containing the metadata that references them is committed. This
prevents a crash from exposing stale data in a newly-allocated block.

---

## Write Queue Ordering Model

Without a journal, ordered writes to disk would have used a four-queue model to ensure no pointer
to a resource is committed before the resource itself exists on disk, and no resource
is freed before the pointer to it is gone:

```
Q1 — new inodes, new data blocks, modified inode table entries
Q2 — inode bitmaps, block bitmaps, group descriptors
Q3 — directory entries being added (new references)
Q4 — directory entries being removed, superblock counts
```

Each queue is fully flushed (and disk write completion confirmed) before the next queue
begins. A block already in queue N that a new operation also needs for queue M:
- M >= N: mutate in place, no wait needed (later queues flush after)
- M < N: wait for the current flush cycle to complete before proceeding

With journaling this complexity goes away — the journal enforces ordering internally
and all mutations within a transaction can be made freely in memory before commit.

---

## Operations

### `write` — append or overwrite file data

**File data (not journaled, written first):**
| Block | Change |
|-------|--------|
| File data blocks | New content written to final location |

**Metadata (journaled):**
| Block | Change |
|-------|--------|
| Inode table | `i_size` if grown, `i_mtime`, `i_ctime`, `i_blocks` if new blocks allocated, `i_block[]` if new blocks allocated |
| Block bitmap | Bits set for any newly allocated blocks |
| Indirect pointer block(s) | New pointer entries if crossing direct/indirect boundary |
| Group descriptor | `bg_free_blocks_count` decremented for each new block |
| Superblock | `s_free_blocks_count` decremented for each new block |

**Notes:**
- Newly allocated data blocks must be zeroed before the journal transaction commits
  (prevents stale data exposure on crash)
- If the write stays within already-allocated blocks, no bitmap or count changes needed
- Indirect pointer blocks are metadata and are journaled even though they support file data

---

### `truncate` — shrink a file

**Metadata (journaled):**
| Block | Change |
|-------|--------|
| Inode table | `i_size`, `i_blocks`, `i_mtime`, `i_ctime`, `i_block[]` pointers cleared |
| Block bitmap | Bits cleared for each freed block |
| Indirect pointer block(s) | Cleared or freed if entire indirect block is released |
| Group descriptor | `bg_free_blocks_count` incremented |
| Superblock | `s_free_blocks_count` incremented |

**Notes:**
- Must process indirect pointer blocks carefully: free the data blocks first, then
  the indirect pointer blocks, then clear the inode's pointer fields
- Truncating to zero frees everything; partial truncate must preserve the last partial block

---

### `create` — new regular file

**Metadata (journaled):**
| Block | Change | Queue |
|-------|--------|-------|
| Inode table | New inode: `i_mode`, `i_uid`, `i_gid`, `i_size=0`, `i_links_count=1`, timestamps | Q1 |
| Inode bitmap | Bit set for new inode | Q2 |
| Block bitmap | Bit set if initial data block allocated | Q2 |
| Group descriptor | `bg_free_inodes_count--`, optionally `bg_free_blocks_count--` | Q2 |
| Parent inode table | `i_mtime`, `i_ctime` updated | Q1 |
| Parent directory data block | New dir entry appended (or new block allocated for parent) | Q3 |
| Superblock | `s_free_inodes_count--`, optionally `s_free_blocks_count--` | Q4 |

---

### `mkdir` — new directory

**Metadata (journaled):**
| Block | Change | Queue |
|-------|--------|-------|
| Inode table | New inode: `i_mode` (`S_IFDIR`), `i_links_count=2`, `i_size=block_size`, timestamps | Q1 |
| New directory data block | `.` entry (ino=self) and `..` entry (ino=parent) | Q1 |
| Inode bitmap | Bit set for new inode | Q2 |
| Block bitmap | Bit set for new directory data block | Q2 |
| Group descriptor | `bg_free_inodes_count--`, `bg_free_blocks_count--`, `bg_used_dirs_count++` | Q2 |
| Parent inode table | `i_links_count++` (the `..` entry is a hard link to parent), `i_mtime`, `i_ctime` | Q1 |
| Parent directory data block | New dir entry | Q3 |
| Superblock | `s_free_inodes_count--`, `s_free_blocks_count--` | Q4 |

**Notes:**
- New inode's `i_links_count = 2`: one for the parent's directory entry, one for its own `.`
- Parent's `i_links_count` is incremented by one because the new dir's `..` is a hard link back

---

### `rmdir` — remove a directory

**Precondition:** directory must be empty (only `.` and `..` entries).

**Metadata (journaled):**
| Block | Change | Queue |
|-------|--------|-------|
| Parent directory data block | Dir entry removed | Q4 |
| Parent inode table | `i_links_count--` (losing the `..` back-reference), `i_mtime`, `i_ctime` | Q1 |
| Inode table | Inode cleared (`i_mode=0`, `i_dtime` set) | Q1 |
| Inode bitmap | Bit cleared for freed inode | Q2 |
| Block bitmap | Bit cleared for freed directory data block | Q2 |
| Group descriptor | `bg_free_inodes_count++`, `bg_free_blocks_count++`, `bg_used_dirs_count--` | Q2 |
| Superblock | `s_free_inodes_count++`, `s_free_blocks_count++` | Q4 |

---

### `unlink` — remove a file (not last link)

**Metadata (journaled):**
| Block | Change | Queue |
|-------|--------|-------|
| Parent directory data block | Dir entry removed | Q4 |
| Parent inode table | `i_mtime`, `i_ctime` | Q1 |
| Inode table | `i_links_count--`, `i_ctime` | Q1 |

---

### `unlink` — remove a file (last link)

**Metadata (journaled):**
| Block | Change | Queue |
|-------|--------|-------|
| Parent directory data block | Dir entry removed | Q4 |
| Parent inode table | `i_mtime`, `i_ctime` | Q1 |
| Inode table | Inode cleared (`i_mode=0`, `i_dtime` set, `i_links_count=0`) | Q1 |
| Inode bitmap | Bit cleared | Q2 |
| Block bitmap | Bits cleared for all data blocks and indirect pointer blocks | Q2 |
| Group descriptor | `bg_free_inodes_count++`, `bg_free_blocks_count++` | Q2 |
| Superblock | `s_free_inodes_count++`, `s_free_blocks_count++` | Q4 |

**Notes:**
- Must walk the full `i_block[]` tree to find all blocks to free, including indirect
  pointer blocks themselves

---

### `link` — create a hard link

**Metadata (journaled):**
| Block | Change | Queue |
|-------|--------|-------|
| Inode table | Target inode `i_links_count++`, `i_ctime` | Q1 |
| Target directory data block | New dir entry | Q3 |
| Target directory inode | `i_mtime`, `i_ctime` | Q1 |

**Notes:**
- Cannot hard-link directories (would create cycles; rejected by convention)
- No new inode or data blocks allocated

---

### `rename` — move within same directory

**Metadata (journaled):**
| Block | Change | Queue |
|-------|--------|-------|
| Directory data block | Entry updated in place (new name, possibly new inode if overwriting) | Q3 |
| Directory inode | `i_mtime`, `i_ctime` | Q1 |
| Old inode (if overwriting) | `i_links_count--`, possibly freed | Q1 |

---

### `rename` — move across directories

The destination entry must be durable before the source entry is removed.
A crash between the two leaves the file under both names — harmless and
fsck-recoverable. A crash before the destination is written leaves the
filesystem unchanged.

**Metadata (journaled):**
| Block | Change | Queue |
|-------|--------|-------|
| Destination dir data block | New dir entry added | Q3 |
| Destination dir inode | `i_mtime`, `i_ctime`, `i_links_count++` if moving a dir | Q1 |
| Source dir data block | Dir entry removed | Q4 |
| Source dir inode | `i_mtime`, `i_ctime`, `i_links_count--` if moving a dir | Q1 |
| Moved inode table | `i_ctime`; if moving a dir, update `..` in its data block | Q1 |
| Moved dir data block | `..` entry updated to point to new parent (if moving a dir) | Q1 |
| Overwritten inode (if any) | `i_links_count--`, possibly freed | Q1 |
| Block bitmap, inode bitmap | Only if overwritten inode was last link | Q2 |
| Group descriptor, superblock | Only if blocks/inodes freed | Q2/Q4 |

---

### `symlink` — create a symbolic link

**Metadata (journaled):**
| Block | Change | Queue |
|-------|--------|-------|
| Inode table | New inode: `i_mode` (`S_IFLNK`), `i_links_count=1`, `i_size=len(target)` | Q1 |
| Inode bitmap | Bit set | Q2 |
| Parent directory data block | New dir entry | Q3 |
| Parent inode table | `i_mtime`, `i_ctime` | Q1 |
| Group descriptor | `bg_free_inodes_count--` | Q2 |
| Superblock | `s_free_inodes_count--` | Q4 |

**Fast symlink (target ≤ 60 bytes):**
| Block | Change |
|-------|--------|
| Inode table | Target string stored directly in `i_block[]` (60 bytes); no data block needed |

**Slow symlink (target > 60 bytes):**
| Block | Change |
|-------|--------|
| Data block | Target string stored in a regular data block |
| Block bitmap | Bit set |
| Group descriptor, superblock | `free_blocks_count--` |

---

### `chmod` — change permissions

**Metadata (journaled):**
| Block | Change |
|-------|--------|
| Inode table | `i_mode` (permission bits only), `i_ctime` |

---

### `chown` — change ownership

**Metadata (journaled):**
| Block | Change |
|-------|--------|
| Inode table | `i_uid`, `i_gid`, `i_ctime` |

---

### `utimes` — update timestamps

**Metadata (journaled):**
| Block | Change |
|-------|--------|
| Inode table | `i_atime`, `i_mtime` (as specified by caller) |

---

## Block Allocation Strategy

When allocating a new block or inode, the allocator should:

1. Prefer the same block group as the parent directory (locality)
2. Scan the block/inode bitmap for the first free bit
3. Set the bit, decrement the group descriptor count, decrement the superblock count
4. Return the absolute block/inode number

New data blocks must be zeroed before they are referenced by any committed metadata,
to prevent stale data exposure after a crash.

---

## Inode Cache Consistency

The `Arc<Inode>` in the inode cache and the raw bytes in the inode table block in the
block cache are two representations of the same data. During write operations:

- Mutations go to the `Arc<Inode>` fields (live view for concurrent readers)
- A `flush_inode()` step serializes the struct back to the inode table block in the
  block cache before that block is added to the journal transaction
- The inode cache entry must not be evictable while its backing block is dirty

---

## Prerequisites for Write Support

In rough implementation order:

1. **`write_block(sector, buf)` on `BlockVolume`** — the physical write path
2. **Dirty flag on `BlockBuf`** — distinguishes modified cache entries
3. **Block allocation** — bitmap scan + free count update
4. **Inode allocation** — bitmap scan + free count update
5. **`flush_inode()`** — serialize `Arc<Inode>` fields back to block cache
6. **Journal** — transaction begin/commit, log area management, crash recovery
7. **Individual operations** — roughly in order: `write`, `create`, `mkdir`,
   `unlink`, `rmdir`, `rename`, `link`, `symlink`, `chmod`/`chown`/`utimes`
