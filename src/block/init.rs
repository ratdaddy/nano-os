//! Block subsystem initialization
//!
//! Handles hardware probing, driver initialization, partition discovery,
//! and volume registration for the block layer.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::block::disk::BlockDisk;
use crate::block::partition;
use crate::block::volume::{BlockVolume, PartitionVolume, WholeDiskVolume};
use crate::drivers::{sd, virtio_blk};
use crate::dtb;
use crate::thread;

// Boot sector constants
const BOOT_SIGNATURE: u16 = 0xAA55;
const BOOT_SIGNATURE_OFFSET: usize = 510;
const SECTOR_SIZE: usize = 512;

// exFAT boot sector offsets
const EXFAT_FILESYSTEM_NAME_OFFSET: usize = 3;
const EXFAT_FILESYSTEM_NAME_LEN: usize = 8;
const EXFAT_CLUSTER_HEAP_OFFSET: usize = 88;
const EXFAT_ROOT_DIR_CLUSTER_OFFSET: usize = 96;
const EXFAT_SECTORS_PER_CLUSTER_SHIFT_OFFSET: usize = 109;
const EXFAT_MAX_CLUSTER_SHIFT: u8 = 25;

// exFAT directory entry constants
const EXFAT_DIR_ENTRY_SIZE: usize = 32;
const EXFAT_ENTRIES_PER_SECTOR: usize = SECTOR_SIZE / EXFAT_DIR_ENTRY_SIZE;
const EXFAT_VOLUME_LABEL_ENTRY_TYPE: u8 = 0x83;
const EXFAT_END_OF_DIRECTORY_MARKER: u8 = 0x00;
const EXFAT_LABEL_CHAR_COUNT_OFFSET: usize = 1;
const EXFAT_LABEL_OFFSET: usize = 2;
const EXFAT_MAX_LABEL_LENGTH: usize = 11;
const UTF16_CHAR_SIZE: usize = 2;

// FAT32 boot sector offsets
const FAT32_FILESYSTEM_TYPE_OFFSET: usize = 82;
const FAT32_FILESYSTEM_TYPE_LEN: usize = 8;
const FAT32_VOLUME_LABEL_OFFSET: usize = 71;
const FAT32_VOLUME_LABEL_LEN: usize = 11;

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

// Static buffers for disk I/O - must be properly aligned for DMA
#[repr(C, align(512))]
struct SectorBuffer([u8; SECTOR_SIZE]);

static mut MBR_BUFFER: SectorBuffer = SectorBuffer([0; SECTOR_SIZE]);
static mut SECTOR_BUFFER: SectorBuffer = SectorBuffer([0; SECTOR_SIZE]);

/// Read exFAT volume label by parsing root directory entries
///
/// # Arguments
/// * `volume` - The partition volume to read from
/// * `boot_sector` - The exFAT boot sector (already read)
///
/// # Returns
/// * `Some(label)` if volume label was found
/// * `None` if no label or error reading directory
fn read_exfat_volume_label(volume: &PartitionVolume, boot_sector: &[u8; 512]) -> Option<String> {
    // Parse boot sector fields
    let cluster_heap_offset = u32::from_le_bytes(
        boot_sector[EXFAT_CLUSTER_HEAP_OFFSET..EXFAT_CLUSTER_HEAP_OFFSET + 4].try_into().unwrap()
    );
    let root_dir_cluster = u32::from_le_bytes(
        boot_sector[EXFAT_ROOT_DIR_CLUSTER_OFFSET..EXFAT_ROOT_DIR_CLUSTER_OFFSET + 4].try_into().unwrap()
    );
    let sectors_per_cluster_shift = boot_sector[EXFAT_SECTORS_PER_CLUSTER_SHIFT_OFFSET];

    // Validate cluster shift (should be reasonable, e.g., 0-8 for 1-256 sectors/cluster)
    if sectors_per_cluster_shift > EXFAT_MAX_CLUSTER_SHIFT {
        return None;
    }

    let sectors_per_cluster = 1u32 << sectors_per_cluster_shift;

    // Calculate root directory LBA
    // Clusters start at 2, so subtract 2 from cluster number
    let cluster_offset = (root_dir_cluster.checked_sub(2)? as u64)
        .checked_mul(sectors_per_cluster as u64)?;
    let root_dir_lba = (cluster_heap_offset as u64).checked_add(cluster_offset)?;

    // Validate LBA is within partition bounds
    if root_dir_lba >= volume.size_blocks() {
        return None;
    }

    // Read first sector of root directory
    let dir_buf = unsafe {
        let buf = &raw mut SECTOR_BUFFER.0;
        &mut *buf
    };

    volume.read_blocks(root_dir_lba, dir_buf).ok()?;

    // Parse directory entries
    for i in 0..EXFAT_ENTRIES_PER_SECTOR {
        let entry_offset = i * EXFAT_DIR_ENTRY_SIZE;
        let entry_type = dir_buf[entry_offset];

        // Volume Label entry type
        if entry_type == EXFAT_VOLUME_LABEL_ENTRY_TYPE {
            let char_count = dir_buf[entry_offset + EXFAT_LABEL_CHAR_COUNT_OFFSET] as usize;
            if char_count > EXFAT_MAX_LABEL_LENGTH {
                continue;  // Invalid label length
            }

            // Volume label is UTF-16 encoded
            let mut label = String::new();
            for j in 0..char_count {
                let utf16_offset = entry_offset + EXFAT_LABEL_OFFSET + (j * UTF16_CHAR_SIZE);
                let code_unit = u16::from_le_bytes(
                    dir_buf[utf16_offset..utf16_offset + UTF16_CHAR_SIZE].try_into().unwrap()
                );

                // Simple UTF-16 to char conversion (handles BMP only)
                if let Some(ch) = char::from_u32(code_unit as u32) {
                    label.push(ch);
                }
            }

            return Some(label);
        }

        // Stop at end of directory marker
        if entry_type == EXFAT_END_OF_DIRECTORY_MARKER {
            break;
        }
    }

    None
}

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
    let mut volumes: Vec<PartitionVolume> = Vec::new();

    for part in partitions {
        kprintln!("\nProbing partition {} filesystem...", part.number);

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
                    kprintln!("  Boot signature: {:#06x}", boot_sig);

                    // Try to identify filesystem
                    if boot_sig == BOOT_SIGNATURE {
                        // Check for exFAT
                        let fs_name = core::str::from_utf8(
                            &buf[EXFAT_FILESYSTEM_NAME_OFFSET..EXFAT_FILESYSTEM_NAME_OFFSET + EXFAT_FILESYSTEM_NAME_LEN]
                        ).unwrap_or("");
                        if fs_name == "EXFAT   " {
                            kprintln!("  Filesystem: exFAT");

                            // Read volume label from root directory
                            if let Some(label) = read_exfat_volume_label(&volume, buf) {
                                if !label.is_empty() {
                                    kprintln!("  Volume label: {}", label);
                                }
                            }
                        } else {
                            // Check for FAT32
                            let fat32_type = core::str::from_utf8(
                                &buf[FAT32_FILESYSTEM_TYPE_OFFSET..FAT32_FILESYSTEM_TYPE_OFFSET + FAT32_FILESYSTEM_TYPE_LEN]
                            ).unwrap_or("");
                            if fat32_type.starts_with("FAT32") {
                                // Try to read volume label
                                let label = core::str::from_utf8(
                                    &buf[FAT32_VOLUME_LABEL_OFFSET..FAT32_VOLUME_LABEL_OFFSET + FAT32_VOLUME_LABEL_LEN]
                                ).unwrap_or("").trim_end();
                                kprintln!("  Filesystem: FAT32");
                                if !label.is_empty() && label != "NO NAME" {
                                    kprintln!("  Volume label: {}", label);
                                }
                            } else {
                                // Generic OEM name
                                let oem = core::str::from_utf8(
                                    &buf[OEM_NAME_OFFSET..OEM_NAME_OFFSET + OEM_NAME_LEN]
                                ).unwrap_or("").trim_end();
                                if !oem.is_empty() {
                                    kprintln!("  OEM/Filesystem: {}", oem);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    kprintln!("  Failed to read boot sector: {:?}", e);
                }
            }
        }

        volumes.push(volume);
    }

    kprintln!("\nCreated {} partition volume(s)", volumes.len());

    // TODO: Register volumes as devices

    kprintln!("Block subsystem initialization complete");
    thread::exit();
}
