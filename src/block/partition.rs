//! Partition table parsing
//!
//! Currently supports MBR (Master Boot Record) partition tables.


/// Parse and display MBR partition table from block 0
pub fn parse_mbr(block0: &[u8; 512]) {
    kprintln!("\n=== Partition Table (MBR) ===");

    // Check boot signature
    let signature = u16::from_le_bytes([block0[510], block0[511]]);
    if signature != 0xAA55 {
        kprintln!("Invalid MBR signature: {:#06x} (expected 0xAA55)", signature);
        kprintln!("This may not be a valid MBR partition table.\n");
        return;
    }

    kprintln!("Valid MBR signature found (0xAA55)");
    kprintln!("\nPartitions:");

    // Parse partition entries
    for i in 0..4 {
        let offset = 446 + i * 16;
        let entry_bytes = &block0[offset..offset + 16];

        let status = entry_bytes[0];
        let partition_type = entry_bytes[4];
        let lba_first = u32::from_le_bytes([
            entry_bytes[8],
            entry_bytes[9],
            entry_bytes[10],
            entry_bytes[11],
        ]);
        let num_sectors = u32::from_le_bytes([
            entry_bytes[12],
            entry_bytes[13],
            entry_bytes[14],
            entry_bytes[15],
        ]);

        // Skip empty partitions
        if partition_type == 0 && num_sectors == 0 {
            continue;
        }

        kprintln!("  Partition {}:", i + 1);
        kprintln!("    Status:      {:#04x} {}",
                 status,
                 if status == 0x80 { "(bootable)" } else { "" });
        kprintln!("    Type:        {:#04x} ({})",
                 partition_type,
                 partition_type_name(partition_type));
        kprintln!("    First LBA:   {} ({:#010x})", lba_first, lba_first);
        kprintln!("    Sectors:     {} ({} MB)",
                 num_sectors,
                 (num_sectors as u64 * 512) / (1024 * 1024));
    }

    kprintln!();
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
