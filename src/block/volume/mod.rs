//! Block volume layer
//!
//! Provides logical address spaces over physical block devices.
//! Volumes can represent:
//! - Whole disks (no translation)
//! - Partitions (with LBA offset)
//! - Logical volumes (future)

use alloc::alloc::{alloc, dealloc, Layout};
use alloc::sync::Arc;
use core::ops::Deref;
use core::ptr::NonNull;
use core::slice;

use crate::drivers::{BlockError, BLOCK_SIZE};

/// Size of one cache unit in bytes. Fixed at 4096 to match the hardware page
/// size and VirtIO DMA alignment requirement.
pub const CACHE_BLOCK_SIZE: usize = 4096;

mod partition;
mod whole_disk;

pub use partition::PartitionVolume;
pub use whole_disk::WholeDiskVolume;

/// A 4096-byte buffer allocated with page alignment.
///
/// The alignment satisfies the VirtIO DMA constraint: a `CACHE_BLOCK_SIZE`-byte
/// buffer must not cross a page boundary, which requires it to start on a page
/// boundary.
///
/// Ownership is managed through `Arc<BlockBuf>`. The actual data allocation is
/// separate from the Arc control block — when the last `Arc` clone is dropped,
/// `BlockBuf::drop` frees the underlying buffer.
pub struct BlockBuf(NonNull<u8>);

// SAFETY: the buffer is heap-allocated and not aliased outside of shared &[u8]
// references obtained through Deref. Sending/sharing Arc<BlockBuf> across
// threads is safe for the same reason Arc<[u8]> would be.
unsafe impl Send for BlockBuf {}
unsafe impl Sync for BlockBuf {}

impl Deref for BlockBuf {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        // SAFETY: ptr is valid for CACHE_BLOCK_SIZE bytes for the lifetime of self.
        unsafe { slice::from_raw_parts(self.0.as_ptr(), CACHE_BLOCK_SIZE) }
    }
}

impl Drop for BlockBuf {
    fn drop(&mut self) {
        let layout = Layout::from_size_align(CACHE_BLOCK_SIZE, CACHE_BLOCK_SIZE)
            .expect("valid layout");
        unsafe { dealloc(self.0.as_ptr(), layout) };
    }
}

/// Block volume trait - logical address space
///
/// Represents a logical block device that may be:
/// - A whole physical disk
/// - A partition within a disk
/// - A logical volume spanning multiple devices
///
/// Volumes translate logical block addresses (LBA) within the volume
/// to physical addresses on underlying block devices.
pub trait BlockVolume: Send + Sync {
    /// Read one or more blocks from the volume.
    ///
    /// # Arguments
    /// * `lba` - Logical block address within this volume (0-based)
    /// * `buf` - Buffer to read into (length must be multiple of BLOCK_SIZE)
    ///
    /// # Buffer Requirements
    /// * Length must be a multiple of BLOCK_SIZE (512 bytes)
    /// * Must meet DMA alignment requirements (see validate_read_buffer)
    ///
    /// # Returns
    /// * `Ok(())` - Read completed successfully
    /// * `Err(BlockError)` - Read failed
    fn read_blocks(&self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError>;

    /// Read one cache-sized block and return a shared reference to its data.
    ///
    /// Returns `Arc<BlockBuf>` — callers read directly from the buffer with no
    /// copy. Dropping the `Arc` signals that the block may be evicted.
    ///
    /// The default implementation allocates a fresh buffer on every call
    /// (correct but uncached). `CachedVolume` overrides this to serve hits
    /// from memory.
    fn get_block(&self, lba: u64) -> Result<Arc<BlockBuf>, BlockError> {
        let layout = Layout::from_size_align(CACHE_BLOCK_SIZE, CACHE_BLOCK_SIZE)
            .expect("valid layout");
        let ptr = unsafe { alloc(layout) };
        assert!(!ptr.is_null(), "BlockBuf: allocation failed");
        let buf = unsafe { slice::from_raw_parts_mut(ptr, CACHE_BLOCK_SIZE) };
        if let Err(e) = self.read_blocks(lba, buf) {
            unsafe { dealloc(ptr, layout) };
            return Err(e);
        }
        Ok(Arc::new(BlockBuf(unsafe { NonNull::new_unchecked(ptr) })))
    }

    /// Get the volume size in blocks
    #[allow(dead_code)]
    fn size_blocks(&self) -> u64;

    /// Get the block size
    #[allow(dead_code)]
    fn block_size(&self) -> u32 {
        BLOCK_SIZE as u32
    }
}
