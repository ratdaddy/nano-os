//! Demo: Read procfs files.

use alloc::string::String;

use crate::file::FileType;
use crate::vfs;

pub fn inspect_procfs() {
    println!("=== Procfs Contents ===");
    println!();

    // Look up /proc inode to observe mount traversal
    println!("/proc inode:");
    match vfs::vfs_lookup("/proc") {
        Ok(inode) => {
            println!("  file_type: {:?}", inode.file_type);
            println!("  len:       {}", inode.len);
            match inode.sb {
                Some(sb) => println!("  fs_type:   {}", sb.fs_type()),
                None => println!("  fs_type:   (unknown)"),
            }
        }
        Err(e) => println!("  Lookup error: {:?}", e),
    }
    println!();

    println!("/proc directory:");
    match vfs::vfs_readdir("/proc") {
        Ok(entries) if entries.is_empty() => println!("  (empty)"),
        Ok(entries) => {
            for entry in &entries {
                let type_char = match entry.file_type {
                    FileType::Directory => 'd',
                    FileType::RegularFile => 'f',
                    FileType::CharDevice => 'c',
                    FileType::BlockDevice => 'b',
                };
                println!("  {} {}", type_char, entry.name);
            }
        }
        Err(e) => println!("  Readdir error: {:?}", e),
    }
    println!();

    println!("/proc/version:");
    match vfs::vfs_open("/proc/version") {
        Ok(mut file) => {
            let mut contents = String::new();
            match vfs::vfs_read_to_string(&mut file, &mut contents) {
                Ok(()) => print!("{}", contents),
                Err(e) => println!("  Read error: {:?}", e),
            }
        }
        Err(e) => println!("  Open error: {:?}", e),
    }
    println!();

    println!("/proc/mounts:");
    match vfs::vfs_open("/proc/mounts") {
        Ok(mut file) => {
            let mut contents = String::new();
            match vfs::vfs_read_to_string(&mut file, &mut contents) {
                Ok(()) if contents.is_empty() => println!("  (empty)"),
                Ok(()) => print!("{}", contents),
                Err(e) => println!("  Read error: {:?}", e),
            }
        }
        Err(e) => println!("  Open error: {:?}", e),
    }
}
