use std::fs;

fn main() {
    println!("**Reading /proc/version");
    let version = fs::read_to_string("/proc/version").expect("Failed to read /proc/version");
    println!("{version}");

    println!("**Reading /proc/filesystems");
    let filesystems = fs::read_to_string("/proc/filesystems").expect("Failed to read /proc/filesystems");
    println!("{filesystems}");

    println!("**Reading /proc/mounts");
    let mounts = fs::read_to_string("/proc/mounts").expect("Failed to read /proc/mounts");
    println!("{mounts}");
    loop {}
}
