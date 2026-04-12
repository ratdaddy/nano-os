//! Single-Producer Single-Consumer (SPSC) lock-free ring buffer.
//!
//! This is a fixed-size circular buffer optimized for the case where one thread
//! produces data and another consumes it. No locks are needed because:
//! - Only the producer writes to `head`
//! - Only the consumer writes to `tail`
//! - Atomic orderings ensure proper synchronization

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

/// A lock-free SPSC ring buffer with compile-time fixed size.
pub struct SpscRing<const N: usize> {
    buf: UnsafeCell<[u8; N]>,
    head: AtomicUsize, // Next write position (producer)
    tail: AtomicUsize, // Next read position (consumer)
}

// SAFETY: SPSC access pattern - producer only writes head, consumer only writes tail.
// The buffer contents are protected by the head/tail synchronization.
unsafe impl<const N: usize> Sync for SpscRing<N> {}

impl<const N: usize> SpscRing<N> {
    /// Create a new empty ring buffer.
    pub const fn new() -> Self {
        Self {
            buf: UnsafeCell::new([0; N]),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire) == self.tail.load(Ordering::Acquire)
    }

    /// Push a slice of bytes into the buffer.
    ///
    /// Returns the number of bytes actually pushed. May be less than `data.len()`
    /// if the buffer doesn't have enough space.
    ///
    /// # Safety
    /// Must only be called from the producer side.
    pub fn push_slice(&self, data: &[u8]) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);

        // Calculate available space (one slot always kept empty to distinguish full from empty)
        let available = (tail + N - head - 1) % N;
        if available == 0 {
            return 0;
        }

        let to_copy = data.len().min(available);

        // Calculate contiguous space from head to end of buffer
        let contiguous = N - head;

        unsafe {
            let buf = &mut *self.buf.get();
            if to_copy <= contiguous {
                // Single contiguous copy
                buf[head..head + to_copy].copy_from_slice(&data[..to_copy]);
            } else {
                // Wrap around: copy to end, then from start
                buf[head..].copy_from_slice(&data[..contiguous]);
                buf[..to_copy - contiguous].copy_from_slice(&data[contiguous..to_copy]);
            }
        }

        self.head.store((head + to_copy) % N, Ordering::Release);
        to_copy
    }

    /// Pop a single byte from the buffer.
    ///
    /// Returns `None` if the buffer is empty.
    ///
    /// # Safety
    /// Must only be called from the consumer side.
    pub fn pop(&self) -> Option<u8> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);

        if head == tail {
            return None;
        }

        let byte = unsafe { (*self.buf.get())[tail] };
        self.tail.store((tail + 1) % N, Ordering::Release);
        Some(byte)
    }
}
