//! ext2 filesystem inspection thread.
//!
//! Waits for KERNEL_READY (set by fs_init once ext2 is mounted on /newroot),
//! then inspects the mounted filesystem via its superblock.

use alloc::sync::Arc;
use core::str::from_utf8;
use core::sync::atomic::Ordering;

use crate::file::{FileType, SeekFrom, SuperBlock};
use crate::fs::ext2::Ext2SuperBlock;
use crate::kernel_main::KERNEL_READY;
use crate::kprintln;
use crate::thread;
use crate::vfs;

const EXT2_MOUNTPOINT: &str = "/newroot";

pub fn init() {
    let t = thread::Thread::new(inspect_thread);
    thread::add(t);
}

fn inspect_thread() {
    while !KERNEL_READY.load(Ordering::Acquire) {
        unsafe { thread::yield_now() };
    }

    let inode = vfs::vfs_lookup(EXT2_MOUNTPOINT).expect("ext2 not mounted on /newroot");
    let sb_dyn = inode.sb.expect("/newroot inode has no superblock");
    // SAFETY: /newroot is always mounted by the ext2 filesystem.
    let sb: &Ext2SuperBlock = unsafe { &*(sb_dyn as *const dyn SuperBlock as *const Ext2SuperBlock) };

    kprintln!("\nReading ext2 filesystem\n");

    // Print superblock info
    if let Some(label) = sb.volume_label() {
        kprintln!("ext2: {} blocks ({} bytes), {} inodes, {} groups ('{}')",
                 sb.blocks_count, sb.block_size(), sb.inodes_count, sb.num_groups(), label);
    } else {
        kprintln!("ext2: {} blocks ({} bytes), {} inodes, {} groups",
                 sb.blocks_count, sb.block_size(), sb.inodes_count, sb.num_groups());
    }
    for (i, group) in sb.groups.iter().enumerate() {
        kprintln!("  Group {}: inode_table={}", i, group.inode_table);
    }
    match (sb.journal_inum, sb.journal_blocks) {
        (Some(inum), Some(blocks)) =>
            kprintln!("ext2: journal inode #{}, {} blocks ({} KB)",
                inum, blocks, blocks * sb.block_size() / 1024),
        (None, _) =>
            kprintln!("ext2: no journal (pure ext2)"),
        _ => {}
    }

    let root = sb.root_inode();
    kprintln!("ext2: root inode: ino={}, type={:?}, len={}", root.ino, root.file_type, root.len);

    let mut root_file = root.fops.open(Arc::clone(&root)).unwrap();
    match root_file.fops.readdir(&mut root_file) {
        Ok(entries) => {
            kprintln!("ext2: root directory ({} entries):", entries.len());
            for entry in &entries {
                let type_char = match entry.file_type {
                    FileType::Directory   => 'd',
                    FileType::RegularFile => 'f',
                    FileType::CharDevice  => 'c',
                    FileType::BlockDevice => 'b',
                };
                kprintln!("  {} {}", type_char, entry.name);
            }
        }
        Err(e) => kprintln!("ext2: readdir failed: {:?}", e),
    }

    match root.iops.lookup(&root, "hello.txt") {
        Ok(inode) => {
            let mut file = inode.fops.open(Arc::clone(&inode)).unwrap();
            let fops = file.fops;

            // Full read from offset 0.
            let mut buf_full = [0u8; 256];
            match fops.read(&mut file, &mut buf_full) {
                Ok(n) => kprintln!("ext2: hello.txt full: {:?}", from_utf8(&buf_full[..n]).unwrap_or("<invalid utf8>")),
                Err(e) => kprintln!("ext2: hello.txt full read failed: {:?}", e),
            }

            // Reopen to reset offset, seek 5 bytes in, then read the next 5.
            let mut file = inode.fops.open(Arc::clone(&inode)).unwrap();
            match fops.seek(&mut file, SeekFrom::Current(5)) {
                Ok(()) => {}
                Err(e) => { kprintln!("ext2: hello.txt seek failed: {:?}", e); return; }
            }
            let mut buf_a = [0u8; 5];
            match fops.read(&mut file, &mut buf_a) {
                Ok(n) => kprintln!("ext2: hello.txt [5..10]: {:?}", from_utf8(&buf_a[..n]).unwrap_or("<invalid utf8>")),
                Err(e) => kprintln!("ext2: hello.txt read [5..10] failed: {:?}", e),
            }

            // Read the remainder of the file.
            let mut buf_b = [0u8; 256];
            match fops.read(&mut file, &mut buf_b) {
                Ok(n) => kprintln!("ext2: hello.txt [10..]: {:?}", from_utf8(&buf_b[..n]).unwrap_or("<invalid utf8>")),
                Err(e) => kprintln!("ext2: hello.txt read [10..] failed: {:?}", e),
            }
        }
        Err(e) => kprintln!("ext2: lookup hello.txt failed: {:?}", e),
    }

    // Single-indirect block test: seek past the 12 direct blocks and read marker text.
    match root.iops.lookup(&root, "large.txt") {
        Ok(inode) => {
            let mut file = inode.fops.open(Arc::clone(&inode)).unwrap();
            let fops = file.fops;
            let indirect_offset = 12 * sb.block_size() as usize;
            match fops.seek(&mut file, SeekFrom::Start(indirect_offset)) {
                Ok(()) => {}
                Err(e) => { kprintln!("ext2: large.txt seek failed: {:?}", e); return; }
            }
            let mut buf = [0u8; 64];
            match fops.read(&mut file, &mut buf) {
                Ok(n) => kprintln!("ext2: large.txt indirect: {:?}", from_utf8(&buf[..n]).unwrap_or("<invalid utf8>")),
                Err(e) => kprintln!("ext2: large.txt read failed: {:?}", e),
            }
        }
        Err(e) => kprintln!("ext2: lookup large.txt failed: {:?}", e),
    }

    // Inode cache verification:
    // Lookups [1] and [2] held simultaneously: same address (cache hit).
    // Lookup [3] after caller drops: same address — LRU Vec still holds strong Arc.
    // Lookup [4] of different inode: miss, inserts lost+found; count grows to 3
    //   (root + hello.txt + lost+found all pinned in LRU Vec).
    {
        let inode1 = root.iops.lookup(&root, "hello.txt");
        let inode2 = root.iops.lookup(&root, "hello.txt");
        if let Ok(i) = &inode1 { kprintln!("ext2: lookup[1] hello.txt: addr={:#x}", Arc::as_ptr(i) as usize); }
        if let Ok(i) = &inode2 { kprintln!("ext2: lookup[2] hello.txt: addr={:#x}", Arc::as_ptr(i) as usize); }
    }
    match root.iops.lookup(&root, "hello.txt") {
        Ok(i) => kprintln!("ext2: lookup[3] hello.txt: addr={:#x}", Arc::as_ptr(&i) as usize),
        Err(e) => kprintln!("ext2: lookup[3] hello.txt failed: {:?}", e),
    }
    match root.iops.lookup(&root, "lost+found") {
        Ok(i) => kprintln!("ext2: lookup[4] lost+found: addr={:#x}", Arc::as_ptr(&i) as usize),
        Err(e) => kprintln!("ext2: lookup[4] lost+found failed: {:?}", e),
    }
    thread::exit();
}
