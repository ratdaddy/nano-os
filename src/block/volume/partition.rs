//! Partition volume implementation
//!
//! Represents a partition within a physical disk with LBA offset translation.

use alloc::sync::Arc;

use crate::block::disk::BlockDisk;
use crate::block::partition::Partition;
use crate::block::volume::BlockVolume;
use crate::drivers::{BlockError, BLOCK_SIZE};

/// A volume representing a single partition on a disk
///
/// Translates volume-relative LBA to disk LBA by adding the partition's
/// starting offset.
pub struct PartitionVolume {
    disk: Arc<BlockDisk>,
    partition: Partition,
}

impl PartitionVolume {
    /// Create a new partition volume
    ///
    /// # Arguments
    /// * `disk` - The underlying block disk (shared via Arc)
    /// * `partition` - Partition metadata (LBA offset, size, etc.)
    pub fn new(disk: Arc<BlockDisk>, partition: Partition) -> Self {
        PartitionVolume { disk, partition }
    }

    /// Get the partition metadata
    #[allow(dead_code)]
    pub fn partition(&self) -> &Partition {
        &self.partition
    }
}

impl BlockVolume for PartitionVolume {
    fn read_blocks(&self, lba: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), BlockError> {
        // Translate volume LBA to disk LBA by adding partition offset
        let disk_lba = self.partition.lba_start as u64 + lba;

        // Bounds check
        if lba >= self.partition.num_sectors as u64 {
            return Err(BlockError::InvalidInput);
        }

        self.disk.read_blocks(disk_lba, buf)
    }

    fn size_blocks(&self) -> u64 {
        self.partition.num_sectors as u64
    }
}
