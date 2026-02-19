//! Whole disk volume implementation
//!
//! Represents the entire physical disk as a single volume with no partitioning.

use alloc::sync::Arc;

use crate::block::disk::BlockDisk;
use crate::block::volume::BlockVolume;
use crate::drivers::BlockError;

/// A volume representing the entire physical disk
///
/// Provides a 1:1 mapping from volume LBA to disk LBA with no translation.
/// This is used for accessing the raw disk before partition detection,
/// or for disks without partition tables.
pub struct WholeDiskVolume {
    disk: Arc<BlockDisk>,
}

impl WholeDiskVolume {
    /// Create a new whole disk volume
    pub fn new(disk: Arc<BlockDisk>) -> Self {
        WholeDiskVolume { disk }
    }
}

impl BlockVolume for WholeDiskVolume {
    fn read_blocks(&self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        // Direct pass-through - no LBA translation needed
        self.disk.read_blocks(lba, buf)
    }

    fn size_blocks(&self) -> u64 {
        // For now, return a placeholder value
        // TODO: Query actual disk size from driver
        0xFFFF_FFFF_FFFF_FFFF
    }
}
