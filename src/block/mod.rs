//! Block layer abstraction
//!
//! Provides device-agnostic block I/O interface with request/completion model.

pub mod device;
pub mod dispatcher;
pub mod partition;

// Re-export main types (will be used when drivers are integrated)
pub use device::{BlockDevice, BlockError};
