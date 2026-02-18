//! Partition table parsing
//!
//! Currently supports MBR (Master Boot Record) partition tables.

use alloc::vec::Vec;

// MBR layout constants
const MBR_SIGNATURE: u16 = 0xAA55;
const MBR_SIGNATURE_OFFSET: usize = 510;
const MBR_PARTITION_TABLE_OFFSET: usize = 446;
const MBR_PARTITION_ENTRY_SIZE: usize = 16;

// Partition entry field offsets
const PARTITION_STATUS_OFFSET: usize = 0;
const PARTITION_TYPE_OFFSET: usize = 4;
const PARTITION_LBA_OFFSET: usize = 8;
const PARTITION_SECTORS_OFFSET: usize = 12;

// Size conversion constants
const SECTOR_SIZE: u64 = 512;
const BYTES_PER_MB: u64 = 1024 * 1024;

/// MBR partition entry
#[derive(Debug, Clone, Copy)]
pub struct Partition {
    /// Partition number (1-4)
    pub number: u8,
    /// Boot indicator (0x80 = bootable, 0x00 = not bootable)
    pub status: u8,
    /// Partition type ID
    pub partition_type: u8,
    /// Starting LBA (Logical Block Address)
    pub lba_start: u32,
    /// Number of sectors in partition
    pub num_sectors: u32,
}

impl Partition {
    /// Check if this partition is bootable
    pub fn is_bootable(&self) -> bool {
        self.status == 0x80
    }

    /// Get partition size in bytes
    pub fn size_bytes(&self) -> u64 {
        self.num_sectors as u64 * SECTOR_SIZE
    }

    /// Get partition size in MB
    pub fn size_mb(&self) -> u64 {
        self.size_bytes() / BYTES_PER_MB
    }

    /// Get human-readable partition type name
    pub fn type_name(&self) -> &'static str {
        partition_type_name(self.partition_type)
    }
}

/// Parse MBR partition table from block 0
///
/// Returns a vector of valid partitions (empty partitions are skipped).
/// Returns an empty vector if the MBR signature is invalid.
pub fn parse_mbr(block0: &[u8; 512]) -> Vec<Partition> {
    let mut partitions = Vec::new();

    // Check boot signature
    let signature = u16::from_le_bytes(
        block0[MBR_SIGNATURE_OFFSET..MBR_SIGNATURE_OFFSET + 2].try_into().unwrap()
    );
    if signature != MBR_SIGNATURE {
        kprintln!("Invalid MBR signature: {:#06x} (expected {:#06x})", signature, MBR_SIGNATURE);
        kprintln!("This may not be a valid MBR partition table.");
        return partitions;
    }

    // Parse partition entries
    for i in 0..4 {
        let offset = MBR_PARTITION_TABLE_OFFSET + i * MBR_PARTITION_ENTRY_SIZE;
        let entry_bytes = &block0[offset..offset + MBR_PARTITION_ENTRY_SIZE];

        let status = entry_bytes[PARTITION_STATUS_OFFSET];
        let partition_type = entry_bytes[PARTITION_TYPE_OFFSET];
        let lba_start = u32::from_le_bytes(
            entry_bytes[PARTITION_LBA_OFFSET..PARTITION_LBA_OFFSET + 4].try_into().unwrap()
        );
        let num_sectors = u32::from_le_bytes(
            entry_bytes[PARTITION_SECTORS_OFFSET..PARTITION_SECTORS_OFFSET + 4].try_into().unwrap()
        );

        // Skip empty partitions
        if partition_type == 0 && num_sectors == 0 {
            continue;
        }

        partitions.push(Partition {
            number: (i + 1) as u8,
            status,
            partition_type,
            lba_start,
            num_sectors,
        });
    }

    partitions
}

/// Get human-readable partition type name
fn partition_type_name(type_id: u8) -> &'static str {
    match type_id {
        0x00 => "Empty",
        0x01 => "FAT12",
        0x04 => "FAT16 <32M",
        0x05 => "Extended",
        0x06 => "FAT16",
        0x07 => "NTFS/exFAT",
        0x0B => "FAT32",
        0x0C => "FAT32 LBA",
        0x0E => "FAT16 LBA",
        0x0F => "Extended LBA",
        0x82 => "Linux swap",
        0x83 => "Linux",
        0x85 => "Linux extended",
        0x8E => "Linux LVM",
        0xEE => "GPT protective",
        0xEF => "EFI System",
        _ => "Unknown",
    }
}
