//! Block device driver interface
//!
//! Hardware-only interface for block storage devices.
//! This trait is implemented by hardware drivers (SDHCI, VirtIO, etc.)
//! and provides asynchronous block I/O operations.

/// Block device driver trait - hardware layer only
///
/// This trait represents the pure hardware interface for block devices.
/// Implementations handle:
/// - Programming hardware registers
/// - Setting up DMA transfers
/// - Handling interrupt acknowledgment
///
/// This trait does NOT handle:
/// - Partition parsing
/// - Request queuing or scheduling
/// - Caching or buffering
/// - Filesystem operations
pub trait BlockDriver: Send {
    /// Get the device name (e.g., "virtio0", "sd0")
    fn name(&self) -> &'static str;

    /// Start a read operation (asynchronous, returns immediately)
    ///
    /// Programs the hardware to read a 512-byte block from the device.
    /// The operation completes asynchronously - an interrupt will fire
    /// when the DMA transfer is complete.
    ///
    /// # Arguments
    /// * `sector` - The logical block address to read from
    /// * `buf` - Buffer to store the 512-byte block (must be DMA-accessible)
    ///
    /// # Returns
    /// * `Ok(())` - Hardware was successfully programmed, read is in progress
    /// * `Err(BlockError)` - Failed to start the read operation
    fn start_read(&mut self, sector: u32, buf: &mut [u8; 512]) -> Result<(), BlockError>;

    /// Get the block size (typically 512 bytes)
    #[allow(dead_code)]
    fn block_size(&self) -> usize {
        512
    }
}

/// Block device error types
#[derive(Debug, Clone, Copy)]
pub enum BlockError {
    #[allow(dead_code)]
    Timeout,
    IoError,
    #[allow(dead_code)]
    InvalidInput,
}

impl core::fmt::Display for BlockError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            BlockError::Timeout => write!(f, "I/O timeout"),
            BlockError::IoError => write!(f, "I/O error"),
            BlockError::InvalidInput => write!(f, "Invalid input"),
        }
    }
}
