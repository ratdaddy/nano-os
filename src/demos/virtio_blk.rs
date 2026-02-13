//! Modern virtio-blk driver (virtio 1.0 / version 2)
//!
//! Structured as a device driver with probe, init, and I/O operations.

use core::ptr::{read_volatile, write_volatile};
use crate::kernel_memory_map::kernel_virt_to_phys;
use super::block_device::{BlockDevice, BlockError};
use super::disk_inspect;

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

// Wrapper types for aligned DMA buffers
// Aligned to next power-of-2 >= size to prevent crossing page boundaries
#[repr(C, align(128))]
struct DescTable([VirtqDesc; 8]);

#[repr(C, align(32))]
struct AvailRing([u16; 12]);

#[repr(C, align(128))]
struct UsedRing([u32; 20]);

#[repr(C, align(16))]
struct ReqBuffer(VirtioBlkReq);

#[repr(C, align(512))]
struct DataBuffer([u8; 512]);

// Queue structures need stable physical addresses for DMA, so keep them as statics
static mut DESC: DescTable = DescTable([VirtqDesc { addr: 0, len: 0, flags: 0, next: 0 }; 8]);
static mut AVAIL: AvailRing = AvailRing([0; 12]);
static mut USED: UsedRing = UsedRing([0; 20]);

// I/O buffers also need stable addresses and proper alignment
static mut REQ: ReqBuffer = ReqBuffer(VirtioBlkReq { req_type: 0, _reserved: 0, sector: 0 });
static mut DATA: DataBuffer = DataBuffer([0; 512]);

// STATUS is only 1 byte, no special alignment needed
static mut STATUS: u8 = 0xFF;

/// virtio-blk device driver
pub struct VirtioBlk {
    base: usize,
    // Physical addresses for I/O buffers (computed once, used on every read)
    req_phys: usize,
    data_phys: usize,
    status_phys: usize,
}

impl VirtioBlk {
    /// Probe for a virtio-blk device on the virtio-mmio bus
    pub fn probe() -> Option<usize> {
        for i in 0..VIRTIO_COUNT {
            let base = VIRTIO_BASE + i * VIRTIO_STRIDE;
            let magic = read32(base, VIRTIO_MMIO_MAGIC_VALUE);
            if magic != 0x74726976 { continue; }

            let version = read32(base, VIRTIO_MMIO_VERSION);
            if version != 2 { continue; }

            let device_id = read32(base, VIRTIO_MMIO_DEVICE_ID);
            if device_id == 2 { // block device
                return Some(base);
            }
        }
        None
    }

    /// Initialize a virtio-blk device at the given base address
    pub fn new(base: usize) -> Result<Self, BlockError> {
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
                return Err(BlockError::IoError);
            }

            // Set up virtqueue 0
            write32(base, VIRTIO_MMIO_QUEUE_SEL, 0);
            let max_size = read32(base, VIRTIO_MMIO_QUEUE_NUM_MAX);
            if max_size == 0 {
                return Err(BlockError::IoError);
            }

            // Use size 8
            write32(base, VIRTIO_MMIO_QUEUE_NUM, 8);

        // Get physical addresses of static queue structures using page table translation
        let desc_phys = kernel_virt_to_phys(core::ptr::addr_of!(DESC) as usize)
            .ok_or(BlockError::IoError)?;
        let avail_phys = kernel_virt_to_phys(core::ptr::addr_of!(AVAIL) as usize)
            .ok_or(BlockError::IoError)?;
        let used_phys = kernel_virt_to_phys(core::ptr::addr_of!(USED) as usize)
            .ok_or(BlockError::IoError)?;

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

        // Compute physical addresses for I/O buffers once
        // These are used on every read, so cache them in the device struct
        let req_phys = kernel_virt_to_phys(core::ptr::addr_of!(REQ) as usize)
            .ok_or(BlockError::IoError)?;
        let data_phys = kernel_virt_to_phys(core::ptr::addr_of!(DATA) as usize)
            .ok_or(BlockError::IoError)?;
        let status_phys = kernel_virt_to_phys(core::ptr::addr_of!(STATUS) as usize)
            .ok_or(BlockError::IoError)?;

        Ok(VirtioBlk { base, req_phys, data_phys, status_phys })
    }
}

impl BlockDevice for VirtioBlk {
    fn read_block(&mut self, sector: u32, buf: &mut [u8; 512]) -> Result<(), BlockError> {
        unsafe {
            // Build request
            REQ.0.req_type = VIRTIO_BLK_T_IN;
            REQ.0._reserved = 0;
            REQ.0.sector = sector as u64;
            STATUS = 0xFF;

            // Use cached physical addresses (computed once during init)
            // Build descriptor chain: req (read) -> data (write) -> status (write)
            DESC.0[0].addr = self.req_phys as u64;
            DESC.0[0].len = 16;
            DESC.0[0].flags = VIRTQ_DESC_F_NEXT;
            DESC.0[0].next = 1;

            DESC.0[1].addr = self.data_phys as u64;
            DESC.0[1].len = 512;
            DESC.0[1].flags = VIRTQ_DESC_F_WRITE | VIRTQ_DESC_F_NEXT;
            DESC.0[1].next = 2;

            DESC.0[2].addr = self.status_phys as u64;
            DESC.0[2].len = 1;
            DESC.0[2].flags = VIRTQ_DESC_F_WRITE;
            DESC.0[2].next = 0;

            // Avail ring: [flags, idx, ring[0..7], used_event]
            let avail_idx = AVAIL.0[1];
            AVAIL.0[2 + (avail_idx as usize % 8)] = 0; // Add descriptor 0
            AVAIL.0[1] = avail_idx.wrapping_add(1);

            // Notify queue 0
            write32(self.base, VIRTIO_MMIO_QUEUE_NOTIFY, 0);

            // Poll for completion
            let start_idx = (USED.0[0] >> 16) as u16;
            for _ in 0..100_000 {
                let current_idx = (USED.0[0] >> 16) as u16;
                if current_idx != start_idx {
                    if STATUS == VIRTIO_BLK_S_OK {
                        // Copy data from static buffer to caller's buffer
                        // Use raw pointer to avoid creating a shared reference to mutable static
                        let data_ptr = core::ptr::addr_of!(DATA.0) as *const [u8; 512];
                        buf.copy_from_slice(&*data_ptr);
                        return Ok(());
                    } else {
                        return Err(BlockError::IoError);
                    }
                }
                core::hint::spin_loop();
            }

            Err(BlockError::Timeout)
        }
    }
}

fn read32(base: usize, offset: usize) -> u32 {
    unsafe { read_volatile((base + offset) as *const u32) }
}

fn write32(base: usize, offset: usize, val: u32) {
    unsafe { write_volatile((base + offset) as *mut u32, val) }
}

pub fn virtio_blk_demo() {
    println!("\n=== virtio-blk Demo (Modern) ===\n");

    // Probe for device
    let base = match VirtioBlk::probe() {
        Some(base) => {
            println!("Found virtio-blk at {:#x}", base);
            base
        }
        None => {
            println!("No virtio-blk device found");
            return;
        }
    };

    // Initialize device
    let mut device = match VirtioBlk::new(base) {
        Ok(dev) => {
            println!("Device initialized successfully");
            dev
        }
        Err(e) => {
            println!("Failed to initialize: {}", e);
            return;
        }
    };

    println!();
    disk_inspect::inspect_disk(&mut device);
    println!();
}
