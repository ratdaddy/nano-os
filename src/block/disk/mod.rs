//! Block disk layer
//!
//! Represents a physical block device with request serialization.

use alloc::boxed::Box;

use crate::drivers::{BlockDriver, BlockError, BLOCK_SIZE};
use crate::thread;

mod dispatcher;
pub use dispatcher::{BlockMessage, request_read_block};

/// A physical block device with request serialization.
///
/// Owns:
/// - The hardware driver (BlockDriver)
/// - Dispatcher thread for serialization
/// - Request queue and in-flight state
pub struct BlockDisk;

impl BlockDisk {
    /// Create a new BlockDisk for the given driver.
    ///
    /// Spawns a dispatcher thread that takes ownership of the driver and
    /// handles all block I/O requests for this disk.
    pub fn new<D: BlockDriver + 'static>(driver: D) -> Result<Self, &'static str> {
        dispatcher::spawn_dispatcher(driver)?;
        Ok(BlockDisk)
    }

    /// Read one or more blocks from the disk.
    ///
    /// Sends a read request to the dispatcher and waits for completion.
    ///
    /// # Arguments
    /// * `lba` - Logical block address to start reading from
    /// * `buf` - Buffer to read into (length must be multiple of BLOCK_SIZE)
    ///
    /// # Requirements
    /// * Buffer length must be a multiple of BLOCK_SIZE (512 bytes)
    /// * Buffer must meet DMA alignment requirements (see validate_read_buffer)
    pub fn read_blocks(&self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        // Send read request to dispatcher
        request_read_block(lba as u32, buf);

        // Wait for response from dispatcher
        let msg = thread::receive_message();
        let response = unsafe { *Box::from_raw(msg.data as *mut BlockMessage) };

        // Check response status
        if let BlockMessage::ReadResponse { status } = response {
            status
        } else {
            Err(BlockError::IoError)
        }
    }

    #[allow(dead_code)]
    pub fn block_size(&self) -> u32 {
        BLOCK_SIZE as u32
    }
}
