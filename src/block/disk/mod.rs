//! Block disk layer
//!
//! Represents a physical block device with request serialization.

use crate::drivers::{BlockDriver, BlockError};

mod dispatcher;
pub use dispatcher::{BlockMessage, request_read_block, send_read_completion};

/// A physical block device with request serialization.
///
/// Owns:
/// - The hardware driver (BlockDriver)
/// - Dispatcher thread for serialization
/// - Request queue and in-flight state
pub struct BlockDisk {
    dispatcher_tid: usize,
}

impl BlockDisk {
    /// Create a new BlockDisk for the given driver.
    ///
    /// Spawns a dispatcher thread that takes ownership of the driver and
    /// handles all block I/O requests for this disk.
    pub fn new<D: BlockDriver + 'static>(driver: D) -> Result<Self, &'static str> {
        let dispatcher_tid = dispatcher::spawn_dispatcher(driver)?;

        Ok(BlockDisk {
            dispatcher_tid,
        })
    }

    /// Request a block read (sends message to dispatcher).
    #[allow(dead_code)] // Will be used by BlockVolume in Step 1.4
    pub fn read_blocks(&self, lba: u64, buf: &mut [u8; 512]) -> Result<(), BlockError> {
        request_read_block(lba as u32, buf);
        Ok(())
    }

    #[allow(dead_code)] // Will be used by BlockVolume in Step 1.4
    pub fn block_size(&self) -> u32 {
        512 // All current drivers use 512
    }

    pub fn dispatcher_tid(&self) -> usize {
        self.dispatcher_tid
    }
}
