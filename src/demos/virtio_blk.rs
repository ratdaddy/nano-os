//! Modern virtio-blk implementation (virtio 1.0 / version 2)
//!
//! Clean rewrite using proper memory allocation and the modern virtio protocol.

use core::ptr::{read_volatile, write_volatile};

// virtio-mmio register offsets (modern, version 2)
const VIRTIO_MMIO_MAGIC_VALUE: usize = 0x000;
const VIRTIO_MMIO_VERSION: usize = 0x004;
const VIRTIO_MMIO_DEVICE_ID: usize = 0x008;
const VIRTIO_MMIO_DEVICE_FEATURES: usize = 0x010;
const VIRTIO_MMIO_DRIVER_FEATURES: usize = 0x020;
const VIRTIO_MMIO_QUEUE_SEL: usize = 0x030;
const VIRTIO_MMIO_QUEUE_NUM_MAX: usize = 0x034;
const VIRTIO_MMIO_QUEUE_NUM: usize = 0x038;
const VIRTIO_MMIO_QUEUE_READY: usize = 0x044;
const VIRTIO_MMIO_QUEUE_NOTIFY: usize = 0x050;
const VIRTIO_MMIO_STATUS: usize = 0x070;
const VIRTIO_MMIO_QUEUE_DESC_LOW: usize = 0x080;
const VIRTIO_MMIO_QUEUE_DESC_HIGH: usize = 0x084;
const VIRTIO_MMIO_QUEUE_AVAIL_LOW: usize = 0x090;
const VIRTIO_MMIO_QUEUE_AVAIL_HIGH: usize = 0x094;
const VIRTIO_MMIO_QUEUE_USED_LOW: usize = 0x0a0;
const VIRTIO_MMIO_QUEUE_USED_HIGH: usize = 0x0a4;

// virtio status bits
const VIRTIO_STATUS_ACKNOWLEDGE: u32 = 1;
const VIRTIO_STATUS_DRIVER: u32 = 2;
const VIRTIO_STATUS_FEATURES_OK: u32 = 8;
const VIRTIO_STATUS_DRIVER_OK: u32 = 4;

// Descriptor flags
const VIRTQ_DESC_F_NEXT: u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;

// virtio-blk request/response
const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_S_OK: u8 = 0;

const VIRTIO_BASE: usize = 0x1000_1000;
const VIRTIO_STRIDE: usize = 0x1000;
const VIRTIO_COUNT: usize = 8;

static mut DEVICE_BASE: Option<usize> = None;
static mut INITIALIZED: bool = false;

// Queue structures (shared between init and read_block)
static mut DESC: [VirtqDesc; 8] = [VirtqDesc { addr: 0, len: 0, flags: 0, next: 0 }; 8];
static mut AVAIL: [u16; 12] = [0; 12]; // flags(1) + idx(1) + ring(8) + event(1) + pad
static mut USED: [u32; 20] = [0; 20];  // flags(1/2) + idx(1/2) + ring(8*2) + event(1/2) + pad

#[repr(C, align(16))]
#[derive(Copy, Clone)]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct VirtioBlkReq {
    req_type: u32,
    _reserved: u32,
    sector: u64,
}

fn read32(base: usize, offset: usize) -> u32 {
    unsafe { read_volatile((base + offset) as *const u32) }
}

fn write32(base: usize, offset: usize, val: u32) {
    unsafe { write_volatile((base + offset) as *mut u32, val) }
}

fn virt_to_phys(virt: usize) -> usize {
    const HIGH_HALF: usize = 0xffff_ffff_0000_0000;
    virt - HIGH_HALF
}

pub fn probe() -> bool {
    println!("Probing for virtio-blk (modern)...");

    unsafe {
        for i in 0..VIRTIO_COUNT {
            let base = VIRTIO_BASE + i * VIRTIO_STRIDE;
            let magic = read32(base, VIRTIO_MMIO_MAGIC_VALUE);
            if magic != 0x74726976 { continue; }

            let version = read32(base, VIRTIO_MMIO_VERSION);
            if version != 2 { continue; }

            let device_id = read32(base, VIRTIO_MMIO_DEVICE_ID);
            if device_id == 2 { // block device
                println!("  Found virtio-blk at {:#x}", base);
                DEVICE_BASE = Some(base);
                return true;
            }
        }
    }
    false
}

pub fn init() -> Result<(), &'static str> {
    let base = unsafe { DEVICE_BASE.ok_or("Device not probed")? };

    unsafe {
        if INITIALIZED {
            return Ok(());
        }

        println!("Initializing virtio-blk...");

        // Reset
        write32(base, VIRTIO_MMIO_STATUS, 0);

        // Ack + Driver
        write32(base, VIRTIO_MMIO_STATUS, VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER);

        // Read features, write 0 (accept defaults)
        let _features = read32(base, VIRTIO_MMIO_DEVICE_FEATURES);
        write32(base, VIRTIO_MMIO_DRIVER_FEATURES, 0);

        // Features OK
        write32(base, VIRTIO_MMIO_STATUS,
            VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_FEATURES_OK);

        // Check features OK was accepted
        let status = read32(base, VIRTIO_MMIO_STATUS);
        if status & VIRTIO_STATUS_FEATURES_OK == 0 {
            return Err("Features not accepted");
        }

        // Set up virtqueue 0
        write32(base, VIRTIO_MMIO_QUEUE_SEL, 0);
        let max_size = read32(base, VIRTIO_MMIO_QUEUE_NUM_MAX);
        if max_size == 0 {
            return Err("Queue not available");
        }

        // Use size 8
        write32(base, VIRTIO_MMIO_QUEUE_NUM, 8);

        // Use the global queue structures
        let desc_phys = virt_to_phys(core::ptr::addr_of!(DESC) as usize);
        let avail_phys = virt_to_phys(core::ptr::addr_of!(AVAIL) as usize);
        let used_phys = virt_to_phys(core::ptr::addr_of!(USED) as usize);

        println!("  Queue physical addresses:");
        println!("    DESC:  {:#x}", desc_phys);
        println!("    AVAIL: {:#x}", avail_phys);
        println!("    USED:  {:#x}", used_phys);

        // Write queue addresses
        write32(base, VIRTIO_MMIO_QUEUE_DESC_LOW, desc_phys as u32);
        write32(base, VIRTIO_MMIO_QUEUE_DESC_HIGH, (desc_phys >> 32) as u32);
        write32(base, VIRTIO_MMIO_QUEUE_AVAIL_LOW, avail_phys as u32);
        write32(base, VIRTIO_MMIO_QUEUE_AVAIL_HIGH, (avail_phys >> 32) as u32);
        write32(base, VIRTIO_MMIO_QUEUE_USED_LOW, used_phys as u32);
        write32(base, VIRTIO_MMIO_QUEUE_USED_HIGH, (used_phys >> 32) as u32);

        // Mark queue ready
        write32(base, VIRTIO_MMIO_QUEUE_READY, 1);

        // Driver OK
        write32(base, VIRTIO_MMIO_STATUS,
            VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_FEATURES_OK | VIRTIO_STATUS_DRIVER_OK);

        INITIALIZED = true;
        println!("  Device ready");
        Ok(())
    }
}

pub fn read_block(sector: u32) -> Result<[u8; 512], &'static str> {
    let base = unsafe { DEVICE_BASE.ok_or("Device not initialized")? };

    unsafe {
        static mut REQ: VirtioBlkReq = VirtioBlkReq { req_type: 0, _reserved: 0, sector: 0 };
        static mut DATA: [u8; 512] = [0; 512];
        static mut STATUS: u8 = 0xFF;

        // Build request
        REQ.req_type = VIRTIO_BLK_T_IN;
        REQ._reserved = 0;
        REQ.sector = sector as u64;
        STATUS = 0xFF;

        let req_phys = virt_to_phys(core::ptr::addr_of!(REQ) as usize);
        let data_phys = virt_to_phys(core::ptr::addr_of!(DATA) as usize);
        let status_phys = virt_to_phys(core::ptr::addr_of!(STATUS) as usize);

        // Build descriptor chain: req (read) -> data (write) -> status (write)
        DESC[0].addr = req_phys as u64;
        DESC[0].len = 16;
        DESC[0].flags = VIRTQ_DESC_F_NEXT;
        DESC[0].next = 1;

        DESC[1].addr = data_phys as u64;
        DESC[1].len = 512;
        DESC[1].flags = VIRTQ_DESC_F_WRITE | VIRTQ_DESC_F_NEXT;
        DESC[1].next = 2;

        DESC[2].addr = status_phys as u64;
        DESC[2].len = 1;
        DESC[2].flags = VIRTQ_DESC_F_WRITE;
        DESC[2].next = 0;

        // Avail ring: [flags, idx, ring[0..7], used_event]
        let avail_idx = AVAIL[1];
        AVAIL[2 + (avail_idx as usize % 8)] = 0; // Add descriptor 0
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        AVAIL[1] = avail_idx.wrapping_add(1);
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        // Notify queue 0
        write32(base, VIRTIO_MMIO_QUEUE_NOTIFY, 0);

        // Poll for completion (Used ring: [flags(u16), idx(u16), ring[](u32,u32), event(u16)])
        let start_idx = (USED[0] >> 16) as u16; // idx is upper 16 bits of USED[0]
        for _ in 0..100_000 {
            core::sync::atomic::fence(core::sync::atomic::Ordering::Acquire);
            let current_idx = (USED[0] >> 16) as u16;
            if current_idx != start_idx {
                if STATUS == VIRTIO_BLK_S_OK {
                    return Ok(DATA);
                } else {
                    return Err("Block read failed");
                }
            }
            core::hint::spin_loop();
        }

        Err("Timeout")
    }
}

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

/// Read and display sector 0 (MBR).
fn read_sector_zero() {
    println!("--- Read Sector 0 (MBR) ---");
    println!();

    match read_block(0) {
        Ok(buf) => {
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
                read_fat32_root(lba);
            }
        }
        Err(e) => {
            println!("Error reading sector 0: {}", e);
        }
    }
}

/// Read and display the FAT32 VBR and root directory.
fn read_fat32_root(part_lba: u32) {
    println!("--- FAT32 Volume Boot Record (sector {}) ---", part_lba);
    println!();

    let vbr = match read_block(part_lba) {
        Ok(buf) => buf,
        Err(e) => {
            println!("Error reading VBR: {}", e);
            return;
        }
    };

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
        let sector_buf = match read_block(root_lba + s) {
            Ok(buf) => buf,
            Err(e) => {
                println!("  Error reading dir sector {}: {}", root_lba + s, e);
                return;
            }
        };

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

pub fn virtio_blk_demo() {
    println!("\n=== virtio-blk Demo (Modern) ===\n");

    if !probe() {
        println!("No virtio-blk device found");
        return;
    }

    if let Err(e) = init() {
        println!("Failed to initialize: {}", e);
        return;
    }

    println!();
    read_sector_zero();
    println!();
}
