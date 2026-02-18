//! Block layer abstraction
//!
//! Provides device-agnostic block I/O interface with request/completion model.

pub mod disk;
pub mod init;
pub mod partition;
pub mod volume;

// Re-export commonly used items
pub use init::init;
