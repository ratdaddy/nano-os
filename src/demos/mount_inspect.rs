//! Demo: Inspect mount table and filesystem registry.

use crate::vfs;

pub fn inspect_mounts() {
    println!("=== Registered Filesystems ===");
    for fs in vfs::filesystems() {
        println!("  {}", fs.name());
    }
    println!();
    println!("=== Mount Table ===");
    for m in vfs::mounts() {
        println!("  {}  {}  {}", m.id, m.fs_type, m.mountpoint);
    }
}
