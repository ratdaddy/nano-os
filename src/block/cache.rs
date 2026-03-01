//! Block cache
//!
//! `CachedVolume` wraps any `BlockVolume` and caches recently-read blocks in
//! memory so that repeated reads of the same LBA skip the disk entirely.

use alloc::sync::Arc;
use spin::Mutex;

use crate::block::volume::{BlockBuf, BlockVolume};
use crate::collections::LruCache;
use crate::drivers::BlockError;

/// Number of blocks held in the cache. 16 × 4KB = 64KB of data.
const CACHE_CAPACITY: usize = 16;

/// A `BlockVolume` wrapper that caches recently-read blocks.
///
/// The inner `LruCache` stores entries in LRU order (front = most recently
/// used). Eviction skips pinned slots — entries whose Arc strong count is
/// greater than 1 (a caller outside the cache holds a reference).
pub struct CachedVolume {
    inner: Arc<dyn BlockVolume>,
    cache: Mutex<LruCache<u64, BlockBuf>>,
}

impl CachedVolume {
    pub fn new(inner: Arc<dyn BlockVolume>) -> Self {
        Self { inner, cache: Mutex::new(LruCache::new(CACHE_CAPACITY)) }
    }
}

impl BlockVolume for CachedVolume {
    fn read_blocks(&self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        self.inner.read_blocks(lba, buf)
    }

    fn get_block(&self, lba: u64) -> Result<Arc<BlockBuf>, BlockError> {
        // Fast path: cache hit — move to front and return a clone.
        if let Some(arc) = self.cache.lock().get(&lba) {
            return Ok(arc);
        }

        // Miss: read outside the lock so the cache isn't held during I/O.
        let buf = self.inner.get_block(lba)?;

        // Insert under the lock, evicting unpinned LRU entries as needed.
        self.cache.lock().insert(lba, Arc::clone(&buf));
        Ok(buf)
    }

    fn size_blocks(&self) -> u64 {
        self.inner.size_blocks()
    }
}
