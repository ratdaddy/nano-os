//! Block device driver interface
//!
//! Common trait and error types for block storage devices.

/// Block device driver interface
pub trait BlockDevice {
    /// Read a 512-byte block from the device
    fn read_block(&mut self, sector: u32, buf: &mut [u8; 512]) -> Result<(), BlockError>;

    /// Get the block size (typically 512 bytes)
    #[allow(dead_code)]
    fn block_size(&self) -> usize { 512 }
}

/// Block device error types
#[derive(Debug)]
pub enum BlockError {
    Timeout,
    IoError,
}

impl core::fmt::Display for BlockError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            BlockError::Timeout => write!(f, "I/O timeout"),
            BlockError::IoError => write!(f, "I/O error"),
        }
    }
}
