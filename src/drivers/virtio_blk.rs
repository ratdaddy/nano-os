//! VirtIO block device driver

use crate::block::disk;
use crate::drivers::{plic, BlockDriver, BlockError};
use crate::kernel_memory_map::kernel_virt_to_phys;
use crate::kernel_trap;

const VIRTIO_BASE: usize = 0x10001000;
const VIRTIO_STRIDE: usize = 0x1000;
const VIRTIO_COUNT: usize = 8;
const VIRTIO_IRQ_BASE: u32 = 1;

// VirtIO device identification
const VIRTIO_MAGIC: u32 = 0x74726976; // "virt" in ASCII
const VIRTIO_VERSION: u32 = 2;
const VIRTIO_DEVICE_ID_BLOCK: u32 = 2;

// VirtIO queue configuration
const VIRTIO_QUEUE_SIZE: u32 = 8;
const VIRTIO_QUEUE_READY_VALUE: u32 = 1;
const VIRTIO_AVAIL_RING_OFFSET: usize = 2;

// VirtIO block request/response sizes
const VIRTIO_BLK_REQ_HEADER_SIZE: u32 = 16;
const VIRTIO_BLK_SECTOR_SIZE: u32 = 512;
const VIRTIO_BLK_STATUS_SIZE: u32 = 1;

// VirtIO interrupt status/ack registers
const VIRTIO_MMIO_INTERRUPT_STATUS: usize = 0x060;
const VIRTIO_MMIO_INTERRUPT_ACK: usize = 0x064;

// VirtIO interrupt status bits
const VIRTIO_INT_USED_BUFFER: u32 = 0x1;

// VirtIO MMIO register offsets
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

// VirtIO status bits
const VIRTIO_STATUS_ACKNOWLEDGE: u32 = 1;
const VIRTIO_STATUS_DRIVER: u32 = 2;
const VIRTIO_STATUS_FEATURES_OK: u32 = 8;
const VIRTIO_STATUS_DRIVER_OK: u32 = 4;

// Descriptor flags
const VIRTQ_DESC_F_NEXT: u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;

// virtio-blk request type
const VIRTIO_BLK_T_IN: u32 = 0;


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

// Aligned DMA buffers
#[repr(C, align(128))]
struct DescTable([VirtqDesc; 8]);

#[repr(C, align(32))]
struct AvailRing([u16; 12]);

#[repr(C, align(16))]
struct ReqBuffer(VirtioBlkReq);

// Static buffers
static mut DESC: DescTable = DescTable([VirtqDesc { addr: 0, len: 0, flags: 0, next: 0 }; 8]);
static mut AVAIL: AvailRing = AvailRing([0; 12]);
static mut REQ: ReqBuffer = ReqBuffer(VirtioBlkReq { req_type: 0, _reserved: 0, sector: 0 });
static mut STATUS: u8 = 0xFF;

/// VirtIO block device
pub struct VirtioBlk {
    base: usize,
    req_phys: usize,
    status_phys: usize,
}

impl VirtioBlk {
    /// Probe for a VirtIO block device
    fn probe() -> Option<usize> {

        kprintln!("VirtIO: Probing for block device...");
        for i in 0..VIRTIO_COUNT {
            let base = VIRTIO_BASE + i * VIRTIO_STRIDE;
            let magic = read32(base, VIRTIO_MMIO_MAGIC_VALUE);
            if magic != VIRTIO_MAGIC {
                continue;
            }

            let version = read32(base, VIRTIO_MMIO_VERSION);
            if version != VIRTIO_VERSION {
                continue;
            }

            let device_id = read32(base, VIRTIO_MMIO_DEVICE_ID);
            kprintln!("VirtIO: Found device at {:#x}, id={}", base, device_id);

            if device_id == VIRTIO_DEVICE_ID_BLOCK {
                kprintln!("VirtIO: Block device found at {:#x}", base);
                return Some(base);
            }
        }
        kprintln!("VirtIO: No block device found");
        None
    }

    /// Create and initialize a new VirtIO block device
    pub fn new() -> Result<Self, BlockError> {

        let base = Self::probe().ok_or(BlockError::IoError)?;

        // Reset device
        write32(base, VIRTIO_MMIO_STATUS, 0);

        // Acknowledge + Driver
        write32(base, VIRTIO_MMIO_STATUS, VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER);

        // Read and accept features
        let features = read32(base, VIRTIO_MMIO_DEVICE_FEATURES);
        kprintln!("VirtIO: device features = {:#x}", features);
        write32(base, VIRTIO_MMIO_DRIVER_FEATURES, 0);

        // Features OK
        write32(base, VIRTIO_MMIO_STATUS,
                VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_FEATURES_OK);

        // Check features OK was accepted
        let status = read32(base, VIRTIO_MMIO_STATUS);
        kprintln!("VirtIO: status after features = {:#x}", status);
        if status & VIRTIO_STATUS_FEATURES_OK == 0 {
            kprintln!("VirtIO: FEATURES_OK not accepted!");
            return Err(BlockError::IoError);
        }

        // Set up virtqueue 0
        write32(base, VIRTIO_MMIO_QUEUE_SEL, 0);
        let max_size = read32(base, VIRTIO_MMIO_QUEUE_NUM_MAX);
        kprintln!("VirtIO: queue max size = {}", max_size);
        if max_size == 0 {
            kprintln!("VirtIO: queue max size is 0!");
            return Err(BlockError::IoError);
        }
        write32(base, VIRTIO_MMIO_QUEUE_NUM, VIRTIO_QUEUE_SIZE);

        // Get physical addresses for queue structures
        let desc_phys = kernel_virt_to_phys(core::ptr::addr_of!(DESC) as usize)
            .ok_or_else(|| {
                kprintln!("VirtIO: Failed to get physical address for DESC");
                BlockError::IoError
            })?;
        let avail_phys = kernel_virt_to_phys(core::ptr::addr_of!(AVAIL) as usize)
            .ok_or_else(|| {
                kprintln!("VirtIO: Failed to get physical address for AVAIL");
                BlockError::IoError
            })?;
        let used_phys = kernel_virt_to_phys(core::ptr::addr_of!(USED) as usize)
            .ok_or_else(|| {
                kprintln!("VirtIO: Failed to get physical address for USED");
                BlockError::IoError
            })?;

        kprintln!("VirtIO: queue physical addresses: desc={:#x}, avail={:#x}, used={:#x}",
                 desc_phys, avail_phys, used_phys);

        // Tell device where the queues are
        write32(base, VIRTIO_MMIO_QUEUE_DESC_LOW, desc_phys as u32);
        write32(base, VIRTIO_MMIO_QUEUE_DESC_HIGH, (desc_phys >> 32) as u32);
        write32(base, VIRTIO_MMIO_QUEUE_AVAIL_LOW, avail_phys as u32);
        write32(base, VIRTIO_MMIO_QUEUE_AVAIL_HIGH, (avail_phys >> 32) as u32);
        write32(base, VIRTIO_MMIO_QUEUE_USED_LOW, used_phys as u32);
        write32(base, VIRTIO_MMIO_QUEUE_USED_HIGH, (used_phys >> 32) as u32);

        // Mark queue as ready
        write32(base, VIRTIO_MMIO_QUEUE_READY, VIRTIO_QUEUE_READY_VALUE);

        // Driver OK - device is now live
        write32(base, VIRTIO_MMIO_STATUS,
                VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER |
                VIRTIO_STATUS_FEATURES_OK | VIRTIO_STATUS_DRIVER_OK);

        // Get physical addresses for I/O buffers
        let req_phys = kernel_virt_to_phys(core::ptr::addr_of!(REQ) as usize)
            .ok_or(BlockError::IoError)?;
        let status_phys = kernel_virt_to_phys(core::ptr::addr_of!(STATUS) as usize)
            .ok_or(BlockError::IoError)?;

        Ok(VirtioBlk {
            base,
            req_phys,
            status_phys,
        })
    }
}

impl BlockDriver for VirtioBlk {
    fn name(&self) -> &'static str {
        let index = (self.base - VIRTIO_BASE) / VIRTIO_STRIDE;
        match index {
            0 => "virtio0",
            1 => "virtio1",
            2 => "virtio2",
            3 => "virtio3",
            4 => "virtio4",
            5 => "virtio5",
            6 => "virtio6",
            7 => "virtio7",
            _ => "virtio?",
        }
    }

    fn start_read(&mut self, sector: u32, buf: &mut [u8; 512]) -> Result<(), BlockError> {

        unsafe {
            // Get physical address of caller's buffer
            let buf_virt = buf.as_ptr() as usize;
            let buf_phys = kernel_virt_to_phys(buf_virt)
                .ok_or(BlockError::IoError)?;

            // Build request
            REQ.0.req_type = VIRTIO_BLK_T_IN;
            REQ.0._reserved = 0;
            REQ.0.sector = sector as u64;
            STATUS = 0xFF;

            // Build descriptor chain (3 descriptors: request header, data buffer, status byte)
            DESC.0[0].addr = self.req_phys as u64;
            DESC.0[0].len = VIRTIO_BLK_REQ_HEADER_SIZE;
            DESC.0[0].flags = VIRTQ_DESC_F_NEXT;
            DESC.0[0].next = 1;

            // Use caller's buffer for DMA
            DESC.0[1].addr = buf_phys as u64;
            DESC.0[1].len = VIRTIO_BLK_SECTOR_SIZE;
            DESC.0[1].flags = VIRTQ_DESC_F_WRITE | VIRTQ_DESC_F_NEXT;
            DESC.0[1].next = 2;

            DESC.0[2].addr = self.status_phys as u64;
            DESC.0[2].len = VIRTIO_BLK_STATUS_SIZE;
            DESC.0[2].flags = VIRTQ_DESC_F_WRITE;
            DESC.0[2].next = 0;

            // Add to available ring
            let avail_idx = AVAIL.0[1];
            AVAIL.0[VIRTIO_AVAIL_RING_OFFSET + (avail_idx as usize % 8)] = 0; // descriptor head index
            AVAIL.0[1] = avail_idx.wrapping_add(1);

            // Set up trap stack for interrupt handling
            let trap_stack = kernel_trap::trap_stack_top();
            core::arch::asm!("csrw sscratch, {}", in(reg) trap_stack);

            // Notify device - interrupt will fire when idle thread enables interrupts
            write32(self.base, VIRTIO_MMIO_QUEUE_NOTIFY, 0);

            // Return immediately - dispatcher will yield and idle thread will enable interrupts
            Ok(())
        }
    }
}

/// VirtIO interrupt handler
fn virtio_irq_handler(_irq: u32) {
    use core::sync::atomic::Ordering;

    let base = DEVICE_BASE.load(Ordering::Relaxed);
    if base == 0 {
        return;
    }

    // Read and acknowledge interrupt
    let int_status = read32(base, VIRTIO_MMIO_INTERRUPT_STATUS);

    if int_status & VIRTIO_INT_USED_BUFFER != 0 {
        // Used buffer notification - request completed
        write32(base, VIRTIO_MMIO_INTERRUPT_ACK, int_status);

        // Send completion message to dispatcher
        disk::send_read_completion(Ok(()));
    }
}

/// Initialize VirtIO block device and register interrupt handler
pub fn init() -> Result<VirtioBlk, BlockError> {
    use core::sync::atomic::Ordering;

    let device = VirtioBlk::new()?;

    // Store device base for interrupt handler
    DEVICE_BASE.store(device.base, Ordering::Relaxed);

    // Calculate device index and IRQ number
    let device_index = ((device.base - VIRTIO_BASE) / VIRTIO_STRIDE) as u32;
    let irq = VIRTIO_IRQ_BASE + device_index;

    kprintln!("VirtIO: Registering IRQ {} for device at {:#x}", irq, device.base);

    // Register interrupt handler with PLIC
    plic::register_irq(irq, virtio_irq_handler);

    Ok(device)
}

// Missing used ring structure
#[repr(C, align(128))]
struct UsedRing([u32; 20]);

static mut USED: UsedRing = UsedRing([0; 20]);

// Device base for interrupt handler
static DEVICE_BASE: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

// MMIO register access
fn read32(base: usize, offset: usize) -> u32 {
    unsafe { ((base + offset) as *const u32).read_volatile() }
}

fn write32(base: usize, offset: usize, val: u32) {
    unsafe { ((base + offset) as *mut u32).write_volatile(val) }
}
