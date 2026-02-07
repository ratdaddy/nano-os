//! Demo: Inspect filesystem through VFS interface.

use crate::file::FileType;
use crate::vfs;

fn list_dir(path: &str) {
    println!("{} directory:", path);
    match vfs::vfs_readdir(path) {
        Ok(entries) => {
            for entry in &entries {
                let type_char = match entry.file_type {
                    FileType::Directory => 'd',
                    FileType::RegularFile => 'f',
                    FileType::CharDevice => 'c',
                };
                match entry.file_type {
                    FileType::CharDevice => {
                        let path = if path == "/" {
                            alloc::format!("/{}", entry.name)
                        } else {
                            alloc::format!("{}/{}", path, entry.name)
                        };
                        let file = vfs::vfs_open(&path).unwrap();
                        let (major, minor) = file.inode.unwrap().rdev().unwrap();
                        println!("  {} {} {}:{}", type_char, entry.name, major, minor);
                    }
                    _ => println!("  {} {}", type_char, entry.name),
                }
            }
        }
        Err(e) => println!("  Error: {:?}", e),
    }
    println!();
}

pub fn inspect_vfs() {
    println!("=== VFS Inspect Demo ===");
    println!();

    list_dir("/");
    list_dir("/dev");

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
