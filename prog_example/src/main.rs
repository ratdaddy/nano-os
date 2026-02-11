use std::fs;

fn main() {
    println!("Reading /proc/version");
    let version = fs::read_to_string("/proc/version").expect("Failed to read /proc/version");
    print!("{version}");
    println!("Reading /proc/mounts");
    let mounts = fs::read_to_string("/proc/mounts").expect("Failed to read /proc/mounts");
    print!("{mounts}");
    loop {}
}
