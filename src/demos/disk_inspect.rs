//! Disk inspection utilities for MBR and FAT32 filesystems
//!
//! Common code for reading and displaying partition tables and FAT32 root directories.

use super::block_device::BlockDevice;

/// Read a little-endian u16 from a byte slice at the given offset.
fn le_u16(buf: &[u8], off: usize) -> u16 {
    (buf[off] as u16) | ((buf[off + 1] as u16) << 8)
}

/// Read a little-endian u32 from a byte slice at the given offset.
fn le_u32(buf: &[u8], off: usize) -> u32 {
    (buf[off] as u32)
        | ((buf[off + 1] as u32) << 8)
        | ((buf[off + 2] as u32) << 16)
        | ((buf[off + 3] as u32) << 24)
}

/// Return a human-readable name for an MBR partition type byte.
fn partition_type_name(t: u8) -> &'static str {
    match t {
        0x00 => "Empty",
        0x01 => "FAT12",
        0x04 => "FAT16 (<32M)",
        0x05 => "Extended",
        0x06 => "FAT16 (>32M)",
        0x07 => "HPFS/NTFS/exFAT",
        0x0B => "FAT32 (CHS)",
        0x0C => "FAT32 (LBA)",
        0x0E => "FAT16 (LBA)",
        0x0F => "Extended (LBA)",
        0x82 => "Linux swap",
        0x83 => "Linux",
        0x85 => "Linux extended",
        0x8E => "Linux LVM",
        0xEE => "GPT protective",
        0xEF => "EFI System",
        _ => "Unknown",
    }
}

/// Format a sector count as a human-readable size string.
fn format_size(sectors: u32) -> (u32, &'static str) {
    let mb = ((sectors as u64) * 512) >> 20;
    if mb >= 1024 {
        // Return GiB * 10 for one decimal place
        let gib_x10 = (mb * 10 + 512) / 1024;
        (gib_x10 as u32, "GiB")
    } else {
        (mb as u32, "MiB")
    }
}

/// Inspect disk structure: read and display MBR, partition table, and FAT32 root directory.
pub fn inspect_disk(device: &mut dyn BlockDevice) {
    println!("--- Read Sector 0 (MBR) ---");
    println!();

    let mut buf = [0u8; 512];
    match device.read_block(0, &mut buf) {
        Ok(()) => {
            // Check MBR signature
            let sig = ((buf[511] as u16) << 8) | (buf[510] as u16);
            if sig != 0xAA55 {
                println!("MBR signature: {:#06x} (INVALID)", sig);
                return;
            }
            println!("MBR signature: {:#06x} (valid)", sig);
            println!();

            // Parse the 4 partition table entries at offset 446 (0x1BE)
            println!("  #  Boot  Type                 Start LBA    Sectors  Size");
            println!("  -  ----  -------------------  ---------  ---------  --------");
            let mut fat32_lba: Option<u32> = None;
            for i in 0..4u8 {
                let base = 446 + (i as usize) * 16;
                let status = buf[base];
                let ptype = buf[base + 4];
                let lba_start = le_u32(&buf, base + 8);
                let sectors = le_u32(&buf, base + 12);

                if ptype == 0x00 {
                    continue;
                }

                // Remember first FAT32 partition
                if fat32_lba.is_none() && (ptype == 0x0B || ptype == 0x0C) {
                    fat32_lba = Some(lba_start);
                }

                let boot = if status == 0x80 { "*" } else { " " };
                let (size_val, size_unit) = format_size(sectors);
                if size_unit == "GiB" {
                    println!("  {}  {}     {:<19}  {:>9}  {:>9}  {}.{} {}",
                        i + 1, boot, partition_type_name(ptype),
                        lba_start, sectors,
                        size_val / 10, size_val % 10, size_unit);
                } else {
                    println!("  {}  {}     {:<19}  {:>9}  {:>9}  {} {}",
                        i + 1, boot, partition_type_name(ptype),
                        lba_start, sectors, size_val, size_unit);
                }
            }

            // Read and parse the FAT32 partition
            if let Some(lba) = fat32_lba {
                println!();
                read_fat32_root(device, lba);
            }
        }
        Err(e) => {
            println!("Error reading sector 0: {}", e);
        }
    }
}

/// Read and display the FAT32 VBR and root directory.
pub fn read_fat32_root(device: &mut dyn BlockDevice, part_lba: u32) {
    println!("--- FAT32 Volume Boot Record (sector {}) ---", part_lba);
    println!();

    let mut vbr = [0u8; 512];
    if let Err(e) = device.read_block(part_lba, &mut vbr) {
        println!("Error reading VBR: {}", e);
        return;
    }

    // Parse BPB fields
    let bytes_per_sector = le_u16(&vbr, 11) as u32;
    let sectors_per_cluster = vbr[13] as u32;
    let reserved_sectors = le_u16(&vbr, 14) as u32;
    let num_fats = vbr[16] as u32;
    let total_sectors = le_u32(&vbr, 32);
    let fat_size = le_u32(&vbr, 36);
    let root_cluster = le_u32(&vbr, 44);

    // Volume label from offset 71 (11 bytes)
    print!("  Volume label:       ");
    for i in 71..82 {
        let b = vbr[i];
        if b >= 0x20 && b < 0x7F { print!("{}", b as char); } else { break; }
    }
    println!();
    println!("  Bytes/sector:       {}", bytes_per_sector);
    println!("  Sectors/cluster:    {} ({} bytes)", sectors_per_cluster,
        bytes_per_sector * sectors_per_cluster);
    println!("  Reserved sectors:   {}", reserved_sectors);
    println!("  FATs:               {}", num_fats);
    println!("  FAT size:           {} sectors", fat_size);
    println!("  Total sectors:      {}", total_sectors);
    println!("  Root cluster:       {}", root_cluster);

    // Calculate where the data region starts
    let data_start = part_lba + reserved_sectors + (num_fats * fat_size);
    // Cluster N maps to: data_start + (N - 2) * sectors_per_cluster
    let root_lba = data_start + (root_cluster - 2) * sectors_per_cluster;
    println!("  Data region start:  sector {}", data_start);
    println!("  Root dir at:        sector {}", root_lba);
    println!();

    // Read the root directory cluster (sectors_per_cluster sectors)
    println!("--- Root Directory ---");
    println!();
    println!("  Name              Attr     Cluster      Size");
    println!("  ----------------  ------  --------  --------");

    let sectors_to_read = sectors_per_cluster.min(8); // cap at one cluster
    for s in 0..sectors_to_read {
        let mut sector_buf = [0u8; 512];
        if let Err(e) = device.read_block(root_lba + s, &mut sector_buf) {
            println!("  Error reading dir sector {}: {}", root_lba + s, e);
            return;
        }

        // 16 directory entries per 512-byte sector
        for entry_idx in 0..16 {
            let off = entry_idx * 32;
            let first = sector_buf[off];

            // 0x00 = no more entries
            if first == 0x00 {
                return;
            }
            // 0xE5 = deleted entry
            if first == 0xE5 {
                continue;
            }

            let attr = sector_buf[off + 11];

            // 0x0F = long file name entry, skip
            if attr == 0x0F {
                continue;
            }

            // Parse 8.3 name
            let name_bytes = &sector_buf[off..off + 8];
            let ext_bytes = &sector_buf[off + 8..off + 11];

            // Build display name: trim trailing spaces, add dot+ext if present
            let mut name = [0u8; 12];
            let mut len = 0;
            for &b in name_bytes {
                if b == b' ' { break; }
                name[len] = b;
                len += 1;
            }
            // Check if extension is non-empty
            if ext_bytes[0] != b' ' {
                name[len] = b'.';
                len += 1;
                for &b in ext_bytes {
                    if b == b' ' { break; }
                    name[len] = b;
                    len += 1;
                }
            }

            let cluster_hi = le_u16(&sector_buf, off + 20) as u32;
            let cluster_lo = le_u16(&sector_buf, off + 26) as u32;
            let cluster = (cluster_hi << 16) | cluster_lo;
            let size = le_u32(&sector_buf, off + 28);

            // Attribute flags
            let mut attr_str = [b'-'; 6];
            if attr & 0x01 != 0 { attr_str[0] = b'R'; } // read-only
            if attr & 0x02 != 0 { attr_str[1] = b'H'; } // hidden
            if attr & 0x04 != 0 { attr_str[2] = b'S'; } // system
            if attr & 0x08 != 0 { attr_str[3] = b'V'; } // volume label
            if attr & 0x10 != 0 { attr_str[4] = b'D'; } // directory
            if attr & 0x20 != 0 { attr_str[5] = b'A'; } // archive

            // Print the name as a string
            print!("  ");
            for i in 0..len {
                print!("{}", name[i] as char);
            }
            // Pad to 18 chars
            for _ in len..18 {
                print!(" ");
            }

            // Print attr, cluster, size
            print!("  ");
            for &b in &attr_str { print!("{}", b as char); }

            if attr & 0x08 != 0 {
                // Volume label — no cluster/size
                println!();
            } else {
                println!("  {:>8}  {:>8}", cluster, size);
            }
        }
    }
}
