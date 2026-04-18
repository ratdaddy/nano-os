//! VirtIO block device driver

use alloc::boxed::Box;
use core::mem::{size_of, transmute};
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use crate::drivers::block::validate_read_buffer;
use crate::drivers::{plic, BlockDriver, BlockError};
use crate::kernel_allocator::alloc_within_page;
use crate::kernel_memory_map::kernel_virt_to_phys;

const VIRTIO_BASE: usize = 0x10001000;
const VIRTIO_STRIDE: usize = 0x1000;
const VIRTIO_COUNT: usize = 8;
const VIRTIO_IRQ_BASE: u32 = 1;

// VirtIO device identification
const VIRTIO_MAGIC: u32 = 0x74726976; // "virt" in ASCII
const VERSION: u32 = 2;
const DEVICE_ID_BLOCK: u32 = 2;

// VirtIO queue configuration
//
// Queue size 4 is the minimum power-of-2 that fits our 3-descriptor chain
// (request header → data buffer → status byte). The dispatcher is single-in-flight
// so we never have more than one chain pending.
const VIRTIO_QUEUE_SIZE: usize = 4;
const QUEUE_READY_VALUE: u32 = 1;
const AVAIL_RING_OFFSET: usize = 2;

// Block request/response sizes
const BLK_REQ_HEADER_SIZE: u32 = size_of::<VirtioBlkReq>() as u32;
const BLK_STATUS_SIZE: u32 = 1;

// MMIO register offsets
const REG_MAGIC_VALUE: usize = 0x000;
const REG_VERSION: usize = 0x004;
const REG_DEVICE_ID: usize = 0x008;
#[cfg(feature = "trace_volumes")]
const REG_DEVICE_FEATURES: usize = 0x010;
const REG_DRIVER_FEATURES: usize = 0x020;
const REG_QUEUE_SEL: usize = 0x030;
const REG_QUEUE_NUM_MAX: usize = 0x034;
const REG_QUEUE_NUM: usize = 0x038;
const REG_QUEUE_READY: usize = 0x044;
const REG_QUEUE_NOTIFY: usize = 0x050;
const REG_INTERRUPT_STATUS: usize = 0x060;
const REG_INTERRUPT_ACK: usize = 0x064;
const REG_STATUS: usize = 0x070;
const REG_QUEUE_DESC_LOW: usize = 0x080;
const REG_QUEUE_DESC_HIGH: usize = 0x084;
const REG_QUEUE_AVAIL_LOW: usize = 0x090;
const REG_QUEUE_AVAIL_HIGH: usize = 0x094;
const REG_QUEUE_USED_LOW: usize = 0x0a0;
const REG_QUEUE_USED_HIGH: usize = 0x0a4;

// Device status bits
const STATUS_ACKNOWLEDGE: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_FEATURES_OK: u32 = 8;
const STATUS_DRIVER_OK: u32 = 4;

// Interrupt status bits
const INT_USED_BUFFER: u32 = 0x1;

// Descriptor flags
const DESC_F_NEXT: u16 = 1;
const DESC_F_WRITE: u16 = 2;

// Block request types
const BLK_T_IN: u32 = 0;
const BLK_T_OUT: u32 = 1;


#[repr(C, align(16))]
#[derive(Debug, Copy, Clone)]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct VirtioBlkReq {
    req_type: u32,
    _reserved: u32,
    sector: u64,
}

type DescTable = [VirtqDesc; VIRTIO_QUEUE_SIZE];
type AvailRing = [u16; VIRTIO_QUEUE_SIZE + 2]; // flags + idx + ring[QUEUE_SIZE]
type UsedRing  = [u32; VIRTIO_QUEUE_SIZE * 2 + 1]; // flags/idx + ring entries

// Device base for interrupt handler — must be a static since the handler has no other
// way to find the device.
static DEVICE_BASE: AtomicUsize = AtomicUsize::new(0);

static COMPLETION_HANDLER: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

/// VirtIO block device
#[derive(Debug)]
pub struct VirtioBlk {
    base: usize,
    desc: Box<DescTable>,
    avail: Box<AvailRing>,
    _used: Box<UsedRing>, // device writes completions here; we detect them via interrupt, not by polling
    req: Box<VirtioBlkReq>,
    status: Box<u8>,
    req_phys: usize,
    status_phys: usize,
}

impl VirtioBlk {
    /// Create and initialize a new VirtIO block device, registering its interrupt handler.
    pub fn new() -> Result<Self, BlockError> {

        let base = Self::probe().ok_or(BlockError::IoError)?;

        // Reset device
        write32(base, REG_STATUS, 0);

        // Acknowledge + Driver
        write32(base, REG_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);

        #[cfg(feature = "trace_volumes")]
        {
            let features = read32(base, REG_DEVICE_FEATURES);
            kprintln!("virtio: device features = {:#x}", features);
        }

        // Decline all optional features — we require only basic block I/O
        write32(base, REG_DRIVER_FEATURES, 0);

        // Features OK
        write32(base, REG_STATUS,
                STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK);

        // Check features OK was accepted
        let status = read32(base, REG_STATUS);
        #[cfg(feature = "trace_volumes")]
        kprintln!("virtio: status after features = {:#x}", status);
        if status & STATUS_FEATURES_OK == 0 {
            kprintln!("VirtIO: FEATURES_OK not accepted!");
            return Err(BlockError::IoError);
        }

        // Set up virtqueue 0
        write32(base, REG_QUEUE_SEL, 0);
        let max_size = read32(base, REG_QUEUE_NUM_MAX);
        #[cfg(feature = "trace_volumes")]
        kprintln!("virtio: queue max size = {}", max_size);
        if max_size == 0 {
            kprintln!("VirtIO: queue max size is 0!");
            return Err(BlockError::IoError);
        }
        write32(base, REG_QUEUE_NUM, VIRTIO_QUEUE_SIZE as u32);

        // Allocate queue structures
        let desc: Box<DescTable> = alloc_within_page();
        let avail: Box<AvailRing> = alloc_within_page();
        let _used: Box<UsedRing> = alloc_within_page();

        // Get physical addresses for queue structures
        let desc_phys = kernel_virt_to_phys(desc.as_ref() as *const DescTable as usize)
            .ok_or_else(|| {
                kprintln!("VirtIO: Failed to get physical address for DESC");
                BlockError::IoError
            })?;
        let avail_phys = kernel_virt_to_phys(avail.as_ref() as *const AvailRing as usize)
            .ok_or_else(|| {
                kprintln!("VirtIO: Failed to get physical address for AVAIL");
                BlockError::IoError
            })?;
        let used_phys = kernel_virt_to_phys(_used.as_ref() as *const UsedRing as usize)
            .ok_or_else(|| {
                kprintln!("VirtIO: Failed to get physical address for USED");
                BlockError::IoError
            })?;

        #[cfg(feature = "trace_volumes")]
        kprintln!("virtio: queue physical addresses: desc={:#x}, avail={:#x}, used={:#x}",
                 desc_phys, avail_phys, used_phys);

        // Tell device where the queues are
        write32(base, REG_QUEUE_DESC_LOW, desc_phys as u32);
        write32(base, REG_QUEUE_DESC_HIGH, (desc_phys >> 32) as u32);
        write32(base, REG_QUEUE_AVAIL_LOW, avail_phys as u32);
        write32(base, REG_QUEUE_AVAIL_HIGH, (avail_phys >> 32) as u32);
        write32(base, REG_QUEUE_USED_LOW, used_phys as u32);
        write32(base, REG_QUEUE_USED_HIGH, (used_phys >> 32) as u32);

        // Mark queue as ready
        write32(base, REG_QUEUE_READY, QUEUE_READY_VALUE);

        // Driver OK - device is now live
        write32(base, REG_STATUS,
                STATUS_ACKNOWLEDGE | STATUS_DRIVER |
                STATUS_FEATURES_OK | STATUS_DRIVER_OK);

        // Allocate I/O buffers and cache their physical addresses for use in start_read
        let req: Box<VirtioBlkReq> = alloc_within_page();
        let status: Box<u8> = alloc_within_page();
        let req_phys = kernel_virt_to_phys(req.as_ref() as *const VirtioBlkReq as usize)
            .ok_or(BlockError::IoError)?;
        let status_phys = kernel_virt_to_phys(status.as_ref() as *const u8 as usize)
            .ok_or(BlockError::IoError)?;

        // Register interrupt handler
        DEVICE_BASE.store(base, Ordering::Relaxed);
        let device_index = ((base - VIRTIO_BASE) / VIRTIO_STRIDE) as u32;
        let irq = VIRTIO_IRQ_BASE + device_index;
        kprintln!("VirtIO block device: IRQ {} at {:#x}", irq, base);
        plic::register_irq(irq, virtio_irq_handler);

        Ok(VirtioBlk { base, desc, avail, _used, req, status, req_phys, status_phys })
    }

    /// Build and submit a VirtIO block request (read or write)
    ///
    /// Shared path for `start_read` and `start_write`. The only differences between
    /// the two operations are the request type and whether the data descriptor is
    /// device-writable (`DESC_F_WRITE`) or device-readable (no flag).
    fn start_io(&mut self, req_type: u32, sector: u32, buf: &[u8], data_flags: u16) -> Result<(), BlockError> {
        // Validate buffer meets DMA requirements
        validate_read_buffer(buf)?;

        // Get physical address of caller's buffer
        let buf_phys = kernel_virt_to_phys(buf.as_ptr() as usize)
            .ok_or(BlockError::IoError)?;

        // Build request header
        self.req.req_type = req_type;
        self.req._reserved = 0;
        self.req.sector = sector as u64;
        *self.status = 0xff;

        // Build descriptor chain (3 descriptors: request header, data buffer, status byte)
        self.desc[0].addr = self.req_phys as u64;
        self.desc[0].len = BLK_REQ_HEADER_SIZE;
        self.desc[0].flags = DESC_F_NEXT;
        self.desc[0].next = 1;

        self.desc[1].addr = buf_phys as u64;
        self.desc[1].len = buf.len() as u32;
        self.desc[1].flags = data_flags;
        self.desc[1].next = 2;

        self.desc[2].addr = self.status_phys as u64;
        self.desc[2].len = BLK_STATUS_SIZE;
        self.desc[2].flags = DESC_F_WRITE;
        self.desc[2].next = 0;

        // Add to available ring
        let avail_idx = self.avail[1];
        self.avail[AVAIL_RING_OFFSET + (avail_idx as usize % VIRTIO_QUEUE_SIZE)] = 0;
        self.avail[1] = avail_idx.wrapping_add(1);

        // Notify device - interrupt will fire when idle thread enables interrupts
        write32(self.base, REG_QUEUE_NOTIFY, 0);

        Ok(())
    }

    /// Probe for a VirtIO block device
    fn probe() -> Option<usize> {

        #[cfg(feature = "trace_volumes")]
        kprintln!("virtio: probing for block device");
        for i in 0..VIRTIO_COUNT {
            let base = VIRTIO_BASE + i * VIRTIO_STRIDE;
            let magic = read32(base, REG_MAGIC_VALUE);
            if magic != VIRTIO_MAGIC {
                continue;
            }

            let version = read32(base, REG_VERSION);
            if version != VERSION {
                continue;
            }

            let device_id = read32(base, REG_DEVICE_ID);
            #[cfg(feature = "trace_volumes")]
            kprintln!("virtio: found device at {:#x}, id={}", base, device_id);

            if device_id == DEVICE_ID_BLOCK {
                return Some(base);
            }
        }
        kprintln!("VirtIO: no block device found");
        None
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

    fn set_completion_handler(&self, handler: fn(Result<(), BlockError>)) {
        COMPLETION_HANDLER.store(handler as *mut (), Ordering::Relaxed);
    }

    fn start_read(&mut self, sector: u32, buf: &mut [u8]) -> Result<(), BlockError> {
        // Device writes into buf — DESC_F_WRITE marks the descriptor as device-writable
        self.start_io(BLK_T_IN, sector, buf, DESC_F_WRITE | DESC_F_NEXT)
    }

    fn start_write(&mut self, sector: u32, buf: &[u8]) -> Result<(), BlockError> {
        // Device reads from buf — no DESC_F_WRITE flag
        self.start_io(BLK_T_OUT, sector, buf, DESC_F_NEXT)
    }
}

/// VirtIO interrupt handler
fn virtio_irq_handler(_irq: u32) {

    let base = DEVICE_BASE.load(Ordering::Relaxed);
    if base == 0 {
        return;
    }

    // Read and acknowledge interrupt
    let int_status = read32(base, REG_INTERRUPT_STATUS);

    if int_status & INT_USED_BUFFER != 0 {
        // Used buffer notification - request completed
        write32(base, REG_INTERRUPT_ACK, int_status);

        let handler_ptr = COMPLETION_HANDLER.load(Ordering::Relaxed);
        if !handler_ptr.is_null() {
            // SAFETY: handler_ptr was stored by set_completion_handler via `handler as *mut ()`.
            // The value is a valid fn(Result<(), BlockError>) pointer; transmute recovers it.
            let handler: fn(Result<(), BlockError>) = unsafe { transmute(handler_ptr) };
            handler(Ok(()));
        }
    }
}

// MMIO register access
fn read32(base: usize, offset: usize) -> u32 {
    unsafe { ((base + offset) as *const u32).read_volatile() }
}

fn write32(base: usize, offset: usize, val: u32) {
    unsafe { ((base + offset) as *mut u32).write_volatile(val) }
}
