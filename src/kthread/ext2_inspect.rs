//! ext2 filesystem inspection thread.
//!
//! Waits for the block subsystem to register the ext2 partition volume, then
//! runs the ext2 inspector. This thread is started by spawn_block_dispatcher()
//! alongside the block init thread; the yield loop ensures it does not attempt
//! to access the volume before block init has registered it.

use crate::dev;
use crate::fs::ext2;
use crate::thread;

/// SCSI disk major and minor for the ext2 partition (sda2).
const EXT2_MAJOR: u32 = 8;
const EXT2_MINOR: u32 = 2;

pub fn init() {
    let t = thread::Thread::new(inspect_thread);
    thread::add(t);
}

fn inspect_thread() {
    let volume = loop {
        match dev::blkdev_get(EXT2_MAJOR, EXT2_MINOR) {
            Ok(v) => break v,
            Err(_) => unsafe { thread::yield_now() },
        }
    };
    ext2::inspect_ext2(volume);
    thread::exit();
}
