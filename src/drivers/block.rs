//! Block device driver interface
//!
//! Hardware-only interface for block storage devices.
//! This trait is implemented by hardware drivers (SDHCI, VirtIO, etc.)
//! and provides asynchronous block I/O operations.

use crate::memory::PAGE_SIZE;

/// Standard block size for block devices (512 bytes)
pub const BLOCK_SIZE: usize = 512;

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
    /// Programs the hardware to read one or more sectors from the device.
    /// The operation completes asynchronously - an interrupt will fire
    /// when the DMA transfer is complete.
    ///
    /// # Arguments
    /// * `sector` - The starting logical block address to read from
    /// * `buf` - Buffer to store the data (must be DMA-accessible and properly aligned)
    ///
    /// # Buffer Requirements
    /// * Length must be a multiple of BLOCK_SIZE (512 bytes)
    /// * Must be 512-byte aligned (for block I/O efficiency)
    /// * **Must not cross page boundaries** (critical for DMA without scatter-gather)
    ///
    /// # Returns
    /// * `Ok(())` - Hardware was successfully programmed, read is in progress
    /// * `Err(BlockError)` - Failed to start the read operation
    ///
    /// Use `validate_read_buffer()` to check buffer requirements before calling.
    fn start_read(&mut self, sector: u32, buf: &mut [u8]) -> Result<(), BlockError>;

    /// Get the block size
    #[allow(dead_code)]
    fn block_size(&self) -> usize {
        BLOCK_SIZE
    }
}

/// Validate buffer for multi-sector reads
///
/// Checks that the buffer meets alignment and size requirements for DMA transfers.
///
/// # Arguments
/// * `buf` - The buffer to validate
///
/// # Returns
/// * `Ok(sector_count)` - Buffer is valid, returns number of sectors
/// * `Err(BlockError::InvalidInput)` - Buffer fails validation
///
/// # Validation Rules
/// 1. Size must be multiple of BLOCK_SIZE (512 bytes)
/// 2. Must be 512-byte aligned (for block I/O efficiency)
/// 3. Must not cross 4KB page boundary (critical for DMA without scatter-gather)
pub fn validate_read_buffer(buf: &[u8]) -> Result<u32, BlockError> {
    let addr = buf.as_ptr() as usize;
    let len = buf.len();

    // Check size is multiple of sector size
    if len == 0 || len % BLOCK_SIZE != 0 {
        return Err(BlockError::InvalidInput);
    }

    // Check 512-byte alignment
    if addr % BLOCK_SIZE != 0 {
        return Err(BlockError::InvalidInput);
    }

    // Critical: verify buffer doesn't cross page boundary
    let start_page = addr / PAGE_SIZE;
    let end_page = (addr + len - 1) / PAGE_SIZE;
    if start_page != end_page {
        return Err(BlockError::InvalidInput);
    }

    // Calculate and return sector count
    let sectors = (len / BLOCK_SIZE) as u32;
    Ok(sectors)
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
