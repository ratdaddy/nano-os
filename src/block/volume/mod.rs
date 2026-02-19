//! Block volume layer
//!
//! Provides logical address spaces over physical block devices.
//! Volumes can represent:
//! - Whole disks (no translation)
//! - Partitions (with LBA offset)
//! - Logical volumes (future)

use crate::drivers::{BlockError, BLOCK_SIZE};

mod partition;
mod whole_disk;

pub use partition::PartitionVolume;
pub use whole_disk::WholeDiskVolume;

/// Block volume trait - logical address space
///
/// Represents a logical block device that may be:
/// - A whole physical disk
/// - A partition within a disk
/// - A logical volume spanning multiple devices
///
/// Volumes translate logical block addresses (LBA) within the volume
/// to physical addresses on underlying block devices.
pub trait BlockVolume {
    /// Read blocks from the volume.
    ///
    /// # Arguments
    /// * `lba` - Logical block address within this volume (0-based)
    /// * `buf` - Buffer to read into (must be BLOCK_SIZE bytes)
    ///
    /// # Returns
    /// * `Ok(())` - Read completed successfully
    /// * `Err(BlockError)` - Read failed
    fn read_blocks(&self, lba: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), BlockError>;

    /// Get the volume size in blocks
    #[allow(dead_code)]
    fn size_blocks(&self) -> u64;

    /// Get the block size
    #[allow(dead_code)]
    fn block_size(&self) -> u32 {
        BLOCK_SIZE as u32
    }
}
