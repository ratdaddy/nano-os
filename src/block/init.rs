//! Block subsystem initialization
//!
//! Handles hardware probing, driver initialization, partition discovery,
//! and volume registration for the block layer.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::block::cache::CachedVolume;
use crate::block::disk::BlockDisk;
use crate::block::partition::{self, Partition};
use crate::block::volume::{BlockVolume, PartitionVolume, WholeDiskVolume};
use crate::dev;
use crate::drivers::{sd, virtio_blk};
use crate::dtb;
use crate::kernel_allocator::alloc_within_page;
use crate::thread;

// Disk I/O buffer size
const SECTOR_SIZE: usize = 512;

// Disk device numbering (Linux SCSI disk major numbers start at 8)
const SCSI_DISK_MAJOR: u32 = 8;

/// Initialize the block subsystem.
///
/// Spawns a thread that will:
/// 1. Probe hardware and initialize the appropriate driver
/// 2. Create BlockDisk with dispatcher thread
/// 3. Read partition table
/// 4. Create BlockVolume instances for each partition
/// 5. Register volumes as devices
///
pub fn init() {
    let t = thread::Thread::new(init_thread);
    thread::add(t);
}

/// Block subsystem initialization thread
fn init_thread() {
    kprintln!("Block subsystem initialization starting...");
    let disk = Arc::new(init_driver());
    let partitions = read_partition_table(&disk);
    let count = register_partitions(&disk, partitions);
    dev::blkdev_register(
        SCSI_DISK_MAJOR, 0, &disk_base_name(SCSI_DISK_MAJOR),
        Arc::new(WholeDiskVolume::new(Arc::clone(&disk))),
    );
    kprintln!("Block subsystem initialization complete ({} partition volumes)", count);
    thread::exit();
}

/// Derive the base device name for a block disk from its major number.
///
/// Linux SCSI disk major numbers start at 8: major 8 → "sda", 9 → "sdb", etc.
fn disk_base_name(major: u32) -> String {
    let letter = (b'a' + major.saturating_sub(SCSI_DISK_MAJOR) as u8) as char;
    format!("sd{}", letter)
}

/// Probe hardware and initialize the appropriate block driver.
fn init_driver() -> BlockDisk {
    match dtb::get_cpu_type() {
        dtb::CpuType::Qemu => {
            BlockDisk::new(virtio_blk::VirtioBlk::new().expect("VirtIO init failed"))
        }
        dtb::CpuType::LicheeRVNano => {
            BlockDisk::new(sd::init().expect("SD init failed"))
        }
        _ => panic!("Unknown CPU type"),
    }
    .expect("Failed to create BlockDisk")
}

/// Read the MBR and return all valid partitions.
fn read_partition_table(disk: &Arc<BlockDisk>) -> Vec<Partition> {
    let whole_disk = WholeDiskVolume::new(Arc::clone(disk));
    let mut buf: Box<[u8; SECTOR_SIZE]> = alloc_within_page();

    #[cfg(feature = "trace_volumes")]
    kprintln!("block: reading partition table from sector 0");

    match whole_disk.read_blocks(0, buf.as_mut()) {
        Ok(()) => {
            #[cfg(feature = "trace_volumes")]
            kprintln!("block: partition table read successfully");
        }
        Err(e) => {
            kprintln!("block: failed to read partition table: {:?}", e);
            thread::exit();
        }
    }

    #[cfg(feature = "trace_volumes")]
    probe::log_mbr_signature(buf.as_ref());

    let partitions = partition::parse_mbr(buf.as_ref());

    if partitions.is_empty() {
        kprintln!("block: no valid partitions found");
    } else {
        #[cfg(feature = "trace_volumes")]
        probe::log_partition_table(&partitions);
    }

    partitions
}

/// Create a CachedVolume for each partition, probe its filesystem (trace only),
/// and register it with the device layer. Returns the number of volumes registered.
fn register_partitions(disk: &Arc<BlockDisk>, partitions: Vec<Partition>) -> usize {
    let mut volumes: Vec<Arc<dyn BlockVolume>> = Vec::new();

    for part in partitions {
        let part_number = part.number;
        let volume = PartitionVolume::new(Arc::clone(disk), part);

        #[cfg(feature = "trace_volumes")]
        probe::probe_filesystem(&volume, part_number);

        let cached: Arc<dyn BlockVolume> = Arc::new(CachedVolume::new(Arc::new(volume)));
        let name = format!("{}{}", disk_base_name(SCSI_DISK_MAJOR), part_number);
        dev::blkdev_register(SCSI_DISK_MAJOR, part_number as u32, &name, Arc::clone(&cached));
        volumes.push(cached);
    }

    volumes.len()
}

/// Filesystem probing for diagnostic output.
///
/// Only compiled when the `trace_volumes` feature is enabled.
/// Reads partition boot sectors and superblocks to identify filesystem types.
#[cfg(feature = "trace_volumes")]
mod probe {
    use core::str::from_utf8;
    use super::*;

    const BOOT_SIGNATURE: u16 = 0xaa55;
    const BOOT_SIGNATURE_OFFSET: usize = 510;

    const FAT32_FILESYSTEM_TYPE_OFFSET: usize = 82;
    const FAT32_FILESYSTEM_TYPE_LEN: usize = 8;
    const FAT32_VOLUME_LABEL_OFFSET: usize = 71;
    const FAT32_VOLUME_LABEL_LEN: usize = 11;

    const EXT2_SUPERBLOCK_SECTOR: u64 = 2;
    const EXT2_MAGIC_OFFSET: usize = 56;
    const EXT2_MAGIC: u16 = 0xef53;

    const OEM_NAME_OFFSET: usize = 3;
    const OEM_NAME_LEN: usize = 8;

    pub fn log_mbr_signature(buf: &[u8; 512]) {
        let signature = u16::from_le_bytes(
            buf[BOOT_SIGNATURE_OFFSET..BOOT_SIGNATURE_OFFSET + 2].try_into().unwrap()
        );
        kprintln!("block: MBR signature: {:#06x}", signature);
    }

    pub fn log_partition_table(partitions: &[Partition]) {
        kprintln!("\n=== Partition Table (MBR) ===");
        for part in partitions {
            kprintln!("  Partition {}: type={:#04x} ({}) lba={} sectors={} ({} MB)",
                     part.number,
                     part.partition_type,
                     part.type_name(),
                     part.lba_start,
                     part.num_sectors,
                     part.size_mb());
        }
        kprintln!();
    }

    pub fn probe_filesystem(volume: &PartitionVolume, part_number: u8) {
        kprintln!("block: probing partition {} filesystem", part_number);

        let mut buf: Box<[u8; SECTOR_SIZE]> = alloc_within_page();
        let buf: &mut [u8; SECTOR_SIZE] = buf.as_mut();

        match volume.read_blocks(0, buf) {
            Ok(()) => {
                let boot_sig = u16::from_le_bytes(
                    buf[BOOT_SIGNATURE_OFFSET..BOOT_SIGNATURE_OFFSET + 2].try_into().unwrap()
                );
                if boot_sig == BOOT_SIGNATURE {
                    let fat32_type = from_utf8(
                        &buf[FAT32_FILESYSTEM_TYPE_OFFSET..FAT32_FILESYSTEM_TYPE_OFFSET + FAT32_FILESYSTEM_TYPE_LEN]
                    ).unwrap_or("");
                    if fat32_type.starts_with("FAT32") {
                        let label = from_utf8(
                            &buf[FAT32_VOLUME_LABEL_OFFSET..FAT32_VOLUME_LABEL_OFFSET + FAT32_VOLUME_LABEL_LEN]
                        ).unwrap_or("").trim_end();
                        if !label.is_empty() && label != "NO NAME" {
                            kprintln!("block: partition {}: FAT32 (\"{}\")", part_number, label);
                        } else {
                            kprintln!("block: partition {}: FAT32", part_number);
                        }
                    } else {
                        let oem = from_utf8(
                            &buf[OEM_NAME_OFFSET..OEM_NAME_OFFSET + OEM_NAME_LEN]
                        ).unwrap_or("").trim_end();
                        kprintln!("block: partition {}: unknown (oem=\"{}\")", part_number, oem);
                    }
                } else {
                    match volume.read_blocks(EXT2_SUPERBLOCK_SECTOR, buf) {
                        Ok(()) => {
                            let ext2_magic = u16::from_le_bytes(
                                buf[EXT2_MAGIC_OFFSET..EXT2_MAGIC_OFFSET + 2].try_into().unwrap()
                            );
                            if ext2_magic == EXT2_MAGIC {
                                kprintln!("block: partition {}: ext2", part_number);
                            } else {
                                kprintln!("block: partition {}: unrecognized filesystem", part_number);
                            }
                        }
                        Err(e) => {
                            kprintln!("block: failed to read partition {} superblock: {:?}", part_number, e);
                        }
                    }
                }
            }
            Err(e) => {
                kprintln!("block: failed to read partition {} boot sector: {:?}", part_number, e);
            }
        }
    }
}

