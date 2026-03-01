//! Collection data structures.

mod lru;
mod spsc_ring;

pub use lru::LruCache;
pub use spsc_ring::SpscRing;
