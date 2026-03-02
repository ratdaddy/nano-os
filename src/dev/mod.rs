pub mod block;
pub mod char;

pub use block::{blkdev_for_each, blkdev_get, blkdev_register};
pub use char::{chrdev_for_each, chrdev_open, chrdev_register};
