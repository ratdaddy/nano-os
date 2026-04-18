# Cache Stampede on Cold Miss

## Problem

`CachedVolume::get_block()` drops the cache lock before issuing a disk read on a
miss, then re-acquires it to insert the result. A second thread requesting the
same block during that window observes a miss, issues a redundant disk read, and
overwrites the first thread's cache entry on insert.

Today this only wastes I/O. Once write support is added, the hazard becomes
correctness-critical: a thread that has dirtied a block and is about to write it
back could have its buffer silently replaced by a stale read from another thread
observing the same cold miss. The stale buffer then becomes the canonical cache
entry and the dirty data is lost.

## Fix

Insert an **in-flight marker** into the cache before releasing the lock on a miss.
A second thread that finds the marker waits for the first read to complete rather
than issuing its own.

This is the pattern Linux uses: the `PG_locked` bit on a page serializes
concurrent fetches of the same page, and waiters block on a page-lock wait queue
until the fetching thread clears the bit and wakes them.

Concrete approach for this codebase:

1. Change the cache value type from `Arc<BlockBuf>` to an enum:
   - `Entry::Ready(Arc<BlockBuf>)` — block is present and usable
   - `Entry::Loading` — a fetch is in progress

2. On a miss:
   - Insert `Entry::Loading` under the lock, then release it
   - Perform the disk read
   - Re-acquire the lock, replace with `Entry::Ready(buf)`, wake waiters

3. A thread that finds `Entry::Loading`:
   - Releases the lock and blocks (e.g., on a condvar or thread message)
   - Is woken by the fetching thread and re-checks the entry

## Priority

Must be resolved before dirty-block writeback is implemented, or a write can
race with a stale reload and silently lose data.
