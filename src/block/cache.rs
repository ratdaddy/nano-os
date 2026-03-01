//! Block cache
//!
//! `CachedVolume` wraps any `BlockVolume` and caches recently-read blocks in
//! memory so that repeated reads of the same LBA skip the disk entirely.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use spin::Mutex;

use crate::block::volume::{BlockBuf, BlockVolume};
use crate::drivers::BlockError;

/// Number of blocks held in the cache. 16 × 4KB = 64KB of data.
const CACHE_CAPACITY: usize = 16;

/// A `BlockVolume` wrapper that caches recently-read blocks.
///
/// Slots are stored in LRU order (front = most recently used). Each entry is
/// `(lba, buffer)`. Eviction skips pinned slots — entries whose Arc strong
/// count is greater than 1 (a caller outside the cache holds a reference).
pub struct CachedVolume {
    inner: Arc<dyn BlockVolume>,
    /// LRU order: front = most recently used.
    slots: Mutex<VecDeque<(u64, Arc<BlockBuf>)>>,
}

impl CachedVolume {
    pub fn new(inner: Arc<dyn BlockVolume>) -> Self {
        Self { inner, slots: Mutex::new(VecDeque::new()) }
    }
}

impl BlockVolume for CachedVolume {
    fn read_blocks(&self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        self.inner.read_blocks(lba, buf)
    }

    fn get_block(&self, lba: u64) -> Result<Arc<BlockBuf>, BlockError> {
        // Fast path: cache hit — move to front and return a clone.
        {
            let mut slots = self.slots.lock();
            if let Some(pos) = slots.iter().position(|(l, _)| *l == lba) {
                let entry = slots.remove(pos).unwrap();
                let arc = Arc::clone(&entry.1);
                slots.push_front(entry);
                return Ok(arc);
            }
        }

        // Miss: read outside the lock.
        let buf = self.inner.get_block(lba)?;

        // Insert, evicting enough unpinned LRU entries to return to capacity.
        // If the cache grew past capacity because all slots were pinned, we
        // try to evict the excess here so subsequent insertions stay at target.
        // Single backward pass: removing from the tail doesn't shift earlier indices.
        let mut slots = self.slots.lock();
        let mut needed = (slots.len() + 1).saturating_sub(CACHE_CAPACITY);
        let mut i = slots.len();
        while needed > 0 && i > 0 {
            i -= 1;
            if Arc::strong_count(&slots[i].1) == 1 {
                slots.remove(i);
                needed -= 1;
            }
        }
        slots.push_front((lba, Arc::clone(&buf)));
        Ok(buf)
    }

    fn size_blocks(&self) -> u64 {
        self.inner.size_blocks()
    }
}
