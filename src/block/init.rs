//! Block subsystem initialization
//!
//! Handles hardware probing, driver initialization, partition discovery,
//! and volume registration for the block layer.

use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::str::from_utf8;

use crate::block::cache::CachedVolume;
use crate::block::disk::BlockDisk;
use crate::block::partition;
use crate::block::volume::{BlockVolume, PartitionVolume, WholeDiskVolume};
use crate::dev;
use crate::drivers::{sd, virtio_blk};
use crate::dtb;
use crate::thread;

// Boot sector constants
const BOOT_SIGNATURE: u16 = 0xaa55;
const BOOT_SIGNATURE_OFFSET: usize = 510;
const SECTOR_SIZE: usize = 512;

// FAT32 boot sector offsets
const FAT32_FILESYSTEM_TYPE_OFFSET: usize = 82;
const FAT32_FILESYSTEM_TYPE_LEN: usize = 8;
const FAT32_VOLUME_LABEL_OFFSET: usize = 71;
const FAT32_VOLUME_LABEL_LEN: usize = 11;

// ext2 superblock offsets (superblock starts at byte 1024 = sector 2)
const EXT2_SUPERBLOCK_SECTOR: u64 = 2;
const EXT2_MAGIC_OFFSET: usize = 56;
const EXT2_MAGIC: u16 = 0xEF53;

// Disk device numbering (Linux SCSI disk major numbers start at 8)
const SCSI_DISK_MAJOR: u32 = 8;

// Generic boot sector offsets
const OEM_NAME_OFFSET: usize = 3;
const OEM_NAME_LEN: usize = 8;

/// Initialize the block subsystem.
///
/// Spawns a thread that will:
/// 1. Probe hardware and initialize the appropriate driver
/// 2. Create BlockDisk with dispatcher thread
/// 3. Read partition table
/// 4. Create BlockVolume instances for each partition
/// 5. Register volumes as devices
///
/// Returns the thread ID of the init thread.
pub fn init() -> Result<usize, &'static str> {
    let t = thread::Thread::new(init_thread);
    let tid = t.id;
    thread::add(t);
    Ok(tid)
}

/// Derive the base device name for a block disk from its major number.
///
/// Linux SCSI disk major numbers start at 8: major 8 → "sda", 9 → "sdb", etc.
fn disk_base_name(major: u32) -> String {
    let letter = (b'a' + major.saturating_sub(SCSI_DISK_MAJOR) as u8) as char;
    format!("sd{}", letter)
}

// Static buffers for disk I/O - must be properly aligned for DMA
#[repr(C, align(512))]
struct SectorBuffer([u8; SECTOR_SIZE]);

static mut MBR_BUFFER: SectorBuffer = SectorBuffer([0; SECTOR_SIZE]);

/// Block subsystem initialization thread
fn init_thread() {
    kprintln!("Block subsystem initialization starting...");

    // Probe hardware and initialize appropriate driver
    let disk = match dtb::get_cpu_type() {
        dtb::CpuType::Qemu => {
            BlockDisk::new(virtio_blk::init().expect("VirtIO init failed"))
        }
        dtb::CpuType::LicheeRVNano => {
            BlockDisk::new(sd::init().expect("SD init failed"))
        }
        _ => panic!("Unknown CPU type"),
    }
    .expect("Failed to create BlockDisk");

    kprintln!("Block dispatcher started (tid={})", disk.dispatcher_tid());

    // Wrap disk in Arc for sharing between volumes
    let disk = Arc::new(disk);

    // Create whole disk volume for reading partition table
    let whole_disk = WholeDiskVolume::new(Arc::clone(&disk));

    // Read partition table through the whole disk volume
    let partitions = unsafe {
        let buf = &raw mut MBR_BUFFER.0;
        let buf = &mut *buf;

        kprintln!("Reading partition table from sector 0...");

        match whole_disk.read_blocks(0, buf) {
            Ok(()) => {
                kprintln!("Partition table read successfully");
            }
            Err(e) => {
                kprintln!("Failed to read partition table: {:?}", e);
                thread::exit();
            }
        }

        // Check MBR signature
        let signature = u16::from_le_bytes(
            buf[BOOT_SIGNATURE_OFFSET..BOOT_SIGNATURE_OFFSET + 2].try_into().unwrap()
        );
        kprintln!("MBR signature: {:#06x} (expected {:#06x})", signature, BOOT_SIGNATURE);

        // Parse partition table
        let partitions = partition::parse_mbr(buf);

        if partitions.is_empty() {
            kprintln!("No valid partitions found");
        } else {
            kprintln!("\n=== Partition Table (MBR) ===");
            kprintln!("Valid MBR signature found (0xAA55)");
            kprintln!("\nPartitions:");

            for part in &partitions {
                kprintln!("  Partition {}:", part.number);
                kprintln!("    Status:      {:#04x} {}",
                         part.status,
                         if part.is_bootable() { "(bootable)" } else { "" });
                kprintln!("    Type:        {:#04x} ({})",
                         part.partition_type,
                         part.type_name());
                kprintln!("    First LBA:   {} ({:#010x})", part.lba_start, part.lba_start);
                kprintln!("    Sectors:     {} ({} MB)",
                         part.num_sectors,
                         part.size_mb());
            }
            kprintln!();
        }

        partitions
    };

    // Create partition volumes and probe filesystems
    let mut volumes: Vec<Arc<dyn BlockVolume>> = Vec::new();

    for part in partitions {
        kprintln!("\nProbing partition {} filesystem...", part.number);

        let part_number = part.number;
        let volume = PartitionVolume::new(Arc::clone(&disk), part);

        // Read first block of partition (boot sector)
        unsafe {
            let buf = &raw mut MBR_BUFFER.0;
            let buf = &mut *buf;

            match volume.read_blocks(0, buf) {
                Ok(()) => {
                    // Check boot signature
                    let boot_sig = u16::from_le_bytes(
                        buf[BOOT_SIGNATURE_OFFSET..BOOT_SIGNATURE_OFFSET + 2].try_into().unwrap()
                    );

                    // Try to identify filesystem
                    if boot_sig == BOOT_SIGNATURE {
                        kprintln!("  Boot signature: {:#06x}", boot_sig);

                        // Check for FAT32
                        let fat32_type = from_utf8(
                            &buf[FAT32_FILESYSTEM_TYPE_OFFSET..FAT32_FILESYSTEM_TYPE_OFFSET + FAT32_FILESYSTEM_TYPE_LEN]
                        ).unwrap_or("");
                        if fat32_type.starts_with("FAT32") {
                            // Try to read volume label
                            let label = from_utf8(
                                &buf[FAT32_VOLUME_LABEL_OFFSET..FAT32_VOLUME_LABEL_OFFSET + FAT32_VOLUME_LABEL_LEN]
                            ).unwrap_or("").trim_end();
                            kprintln!("  Filesystem: FAT32");
                            if !label.is_empty() && label != "NO NAME" {
                                kprintln!("  Volume label: {}", label);
                            }
                        } else {
                            // Generic OEM name
                            let oem = from_utf8(
                                &buf[OEM_NAME_OFFSET..OEM_NAME_OFFSET + OEM_NAME_LEN]
                            ).unwrap_or("").trim_end();
                            if !oem.is_empty() {
                                kprintln!("  OEM/Filesystem: {}", oem);
                            }
                        }
                    } else {
                        // No boot signature - might be ext2 or other filesystem
                        // Try reading ext2 superblock at sector 2 (byte offset 1024)
                        match volume.read_blocks(EXT2_SUPERBLOCK_SECTOR, buf) {
                            Ok(()) => {
                                let ext2_magic = u16::from_le_bytes(
                                    buf[EXT2_MAGIC_OFFSET..EXT2_MAGIC_OFFSET + 2].try_into().unwrap()
                                );
                                if ext2_magic == EXT2_MAGIC {
                                    kprintln!("  Filesystem: ext2");
                                } else {
                                    kprintln!("  Unknown filesystem (no boot sig, ext2 magic not found)");
                                }
                            }
                            Err(e) => {
                                kprintln!("  Failed to read ext2 superblock: {:?}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    kprintln!("  Failed to read boot sector: {:?}", e);
                }
            }
        }

        let cached: Arc<dyn BlockVolume> = Arc::new(CachedVolume::new(Arc::new(volume)));
        let name = format!("{}{}", disk_base_name(SCSI_DISK_MAJOR), part_number);
        dev::blkdev_register(SCSI_DISK_MAJOR, part_number as u32, &name, Arc::clone(&cached));
        volumes.push(cached);
    }

    // Register the whole disk
    let disk_name = disk_base_name(SCSI_DISK_MAJOR);
    dev::blkdev_register(SCSI_DISK_MAJOR, 0, &disk_name, Arc::new(WholeDiskVolume::new(Arc::clone(&disk))));

    kprintln!("\nCreated {} partition volume(s)", volumes.len());

    kprintln!("\nBlock subsystem initialization complete");
    thread::exit();
}
