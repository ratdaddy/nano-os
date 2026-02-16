//! Block layer abstraction
//!
//! Provides device-agnostic block I/O interface with request/completion model.

pub mod disk;
pub mod partition;

// Re-export commonly used items
pub use disk::BlockDisk;
