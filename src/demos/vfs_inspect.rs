//! Demo: Inspect filesystem through VFS interface.

use crate::vfs;

pub fn inspect_vfs() {
    println!("=== VFS Inspect Demo ===");
    println!();

    // List root directory
    println!("Root directory (via vfs_readdir):");
    match vfs::vfs_readdir("/") {
        Ok(entries) => {
            for (name, len, is_dir) in entries {
                let type_char = if is_dir { 'd' } else { 'f' };
                println!("  {} {:>6} {}", type_char, len, name);
            }
        }
        Err(e) => println!("  Error: {:?}", e),
    }
    println!();

    // Open and read /etc/motd
    println!("Reading /etc/motd via vfs_open:");
    match vfs::vfs_open("/etc/motd") {
        Ok(mut file) => {
            let mut contents = alloc::string::String::new();
            match vfs::vfs_read_to_string(&mut file, &mut contents) {
                Ok(()) => println!("{}", contents),
                Err(e) => println!("  Read error: {:?}", e),
            }
        }
        Err(e) => println!("  Open error: {:?}", e),
    }
}
