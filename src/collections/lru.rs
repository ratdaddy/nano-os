//! Generic LRU cache
//!
//! `LruCache<K, V>` stores up to `capacity` entries in LRU order. Entries
//! whose `Arc` strong count is greater than 1 are considered pinned and are
//! skipped during eviction — they will be evicted on a future insert once all
//! external holders have dropped their clones.

use alloc::collections::VecDeque;
use alloc::sync::Arc;

/// A fixed-capacity LRU cache mapping keys to `Arc<V>` values.
///
/// Entries are stored in a `VecDeque` in LRU order (front = most recently
/// used). On a hit, the entry is moved to the front. On a miss, the caller
/// inserts via `insert`; unpinned LRU entries are evicted as needed to stay
/// within capacity.
///
/// An entry is considered pinned when `Arc::strong_count > 1` — some caller
/// outside the cache holds a reference. Pinned entries are skipped during
/// eviction. If all entries are pinned when a new entry is inserted, the cache
/// temporarily exceeds capacity; the excess is recovered on the next insert
/// once entries become unpinned.
pub struct LruCache<K, V> {
    slots: VecDeque<(K, Arc<V>)>,
    capacity: usize,
}

impl<K: Eq, V> LruCache<K, V> {
    pub fn new(capacity: usize) -> Self {
        Self { slots: VecDeque::new(), capacity }
    }

    /// Look up an entry by key.
    ///
    /// On a hit, moves the entry to the front (most recently used) and returns
    /// a clone of the `Arc`. Returns `None` on a miss.
    pub fn get(&mut self, key: &K) -> Option<Arc<V>> {
        let pos = self.slots.iter().position(|(k, _)| k == key)?;
        let entry = self.slots.remove(pos).unwrap();
        let arc = Arc::clone(&entry.1);
        self.slots.push_front(entry);
        Some(arc)
    }

    /// Insert an entry, evicting unpinned LRU entries as needed.
    ///
    /// Performs a single backward pass over the slots, evicting unpinned
    /// entries (strong count == 1) until the cache would be within capacity
    /// after the new entry is added, or until all remaining entries are pinned.
    pub fn insert(&mut self, key: K, value: Arc<V>) {
        let mut needed = (self.slots.len() + 1).saturating_sub(self.capacity);
        let mut i = self.slots.len();
        while needed > 0 && i > 0 {
            i -= 1;
            if Arc::strong_count(&self.slots[i].1) == 1 {
                self.slots.remove(i);
                needed -= 1;
            }
        }
        self.slots.push_front((key, value));
    }
}
