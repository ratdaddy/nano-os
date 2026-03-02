//! Filesystem initialization thread.
//!
//! Mounts the ext2 root filesystem on /newroot, then signals KERNEL_READY
//! so that threads waiting on a fully-initialized filesystem can proceed.

use core::sync::atomic::Ordering;

use crate::kernel_main::KERNEL_READY;
use crate::thread;
use crate::vfs;

const EXT2_DEV: &str = "/dev/sda2";
const EXT2_MOUNTPOINT: &str = "/newroot";

pub fn init() {
    let t = thread::Thread::new(fs_init_thread);
    thread::add(t);
}

fn fs_init_thread() {
    loop {
        match vfs::vfs_mount_at(Some(EXT2_DEV), EXT2_MOUNTPOINT, "ext2") {
            Ok(()) => break,
            Err(_) => unsafe { thread::yield_now() },
        }
    }
    KERNEL_READY.store(true, Ordering::Release);
    thread::exit();
}
