//! SD card driver using ADMA2 (Advanced DMA v2)
//!
//! Fully interrupt-driven implementation for the LicheeRV Nano (SG2002).

use alloc::boxed::Box;
use core::mem::transmute;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::drivers::block::{validate_read_buffer, BLOCK_SIZE};
use crate::drivers::{plic, BlockDriver, BlockError};
use crate::dtb;
use crate::kernel_allocator::alloc_within_page;
use crate::kernel_memory_map::kernel_virt_to_phys;

const SD_BASE: usize = 0x0431_0000;
const SD_IRQ: u32 = 36;

// SDHCI register offsets
const REG_BLOCK_SIZE: usize = 0x04;
const REG_BLOCK_COUNT: usize = 0x06;
const REG_ARGUMENT: usize = 0x08;
const REG_XFER_MODE: usize = 0x0c;
const REG_HOST_CONTROL: usize = 0x28;
const REG_DATA_TIMEOUT: usize = 0x2e;
const REG_NORMAL_INT_STATUS: usize = 0x30;
const REG_ERROR_INT_STATUS: usize = 0x32;
const REG_NORMAL_INT_STATUS_EN: usize = 0x34;
const REG_ERROR_INT_STATUS_EN: usize = 0x36;
const REG_NORMAL_INT_SIGNAL_EN: usize = 0x38;
const REG_ERROR_INT_SIGNAL_EN: usize = 0x3a;
const REG_HOST_CONTROL2: usize = 0x3e;
const REG_ADMA_ADDR_LOW: usize = 0x58;
const REG_ADMA_ADDR_HIGH: usize = 0x5c;

// Data timeout value
const DATA_TIMEOUT_VALUE: u8 = 0x0e;

// Transfer Mode register bits (organized high to low)
const XFER_MODE_DMA_ENABLE: u16 = 1 << 0;
const XFER_MODE_BLOCK_COUNT_EN: u16 = 1 << 1;
const XFER_MODE_AUTO_CMD12_EN: u16 = 1 << 2;
const XFER_MODE_DATA_DIR_READ: u16 = 1 << 4;
const XFER_MODE_MULTI_BLOCK: u16 = 1 << 5;

// Command register bits (organized high to low)
const CMD_INDEX_SHIFT: u16 = 8;
const CMD_DATA_PRESENT: u16 = 1 << 5;
const CMD_CRC_CHECK: u16 = 1 << 4;
const CMD_INDEX_CHECK: u16 = 1 << 3;
const CMD_RESP_TYPE_R1: u16 = 1 << 1;

// SD commands
const SD_CMD17_READ_SINGLE: u16 = 17;
const SD_CMD18_READ_MULTIPLE: u16 = 18;
const SD_CMD24_WRITE_SINGLE: u16 = 24;
const SD_CMD25_WRITE_MULTIPLE: u16 = 25;

// Normal Interrupt Status bits
const INT_TRANSFER_COMPLETE: u16 = 1 << 1;

// Host Control register bits
const HOST_CTRL_DMA_MASK: u8 = 0x18;
const HOST_CTRL_ADMA2_64: u8 = 0x18;

// Host Control 2 register bits
const HOST_CTRL2_64BIT_ADDR: u16 = 1 << 13;

// ADMA2 descriptor attributes (organized high to low)
const ADMA2_ACT_TRAN: u16 = 2 << 4;
const ADMA2_END: u16 = 1 << 1;
const ADMA2_VALID: u16 = 1 << 0;

/// ADMA2 descriptor (64-bit addressing mode)
#[repr(C, align(8))]
#[derive(Debug, Copy, Clone)]
struct Adma2Desc {
    attr: u16,
    length: u16,
    addr_lo: u32,
    addr_hi: u32,
    _reserved: u32,
}

impl Adma2Desc {
    fn new(addr: u64, length: u16, is_end: bool) -> Self {
        let mut attr = ADMA2_VALID | ADMA2_ACT_TRAN;
        if is_end {
            attr |= ADMA2_END;
        }
        Self {
            attr,
            length,
            addr_lo: addr as u32,
            addr_hi: (addr >> 32) as u32,
            _reserved: 0,
        }
    }
}

static COMPLETION_HANDLER: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

/// SD card driver using ADMA2
#[derive(Debug)]
pub struct SdCardAdma {
    desc_table: Box<Adma2Desc>,
    desc_table_phys: usize,
}

impl SdCardAdma {
    pub fn new() -> Result<Self, BlockError> {
        let desc_table: Box<Adma2Desc> = alloc_within_page();
        let desc_table_phys = kernel_virt_to_phys(desc_table.as_ref() as *const Adma2Desc as usize)
            .ok_or(BlockError::IoError)?;

        // Enable interrupt status bits
        write16(REG_NORMAL_INT_STATUS_EN, 0xffff);
        write16(REG_ERROR_INT_STATUS_EN, 0xffff);

        // Configure HOST_CONTROL2 for 64-bit addressing
        let mut host_ctrl2 = read16(REG_HOST_CONTROL2);
        host_ctrl2 |= HOST_CTRL2_64BIT_ADDR;
        write16(REG_HOST_CONTROL2, host_ctrl2);

        // Configure HOST_CONTROL for ADMA2 64-bit mode
        let mut host_ctrl = read8(REG_HOST_CONTROL);
        host_ctrl = (host_ctrl & !HOST_CTRL_DMA_MASK) | HOST_CTRL_ADMA2_64;
        write8(REG_HOST_CONTROL, host_ctrl);

        kprintln!("SD ADMA: Registering IRQ {} for device at {:#x}", SD_IRQ, SD_BASE);
        plic::register_irq(SD_IRQ, sd_irq_handler);

        Ok(SdCardAdma { desc_table, desc_table_phys })
    }

    /// Shared path for start_read and start_write.
    ///
    /// `xfer_dir` is `XFER_MODE_DATA_DIR_READ` for reads, `0` for writes.
    /// `cmd_single` and `cmd_multi` are the SD command indices for single- and
    /// multi-block transfers respectively.
    fn start_io(&mut self, sector: u32, buf: &[u8], xfer_dir: u16, cmd_single: u16, cmd_multi: u16) -> Result<(), BlockError> {
        let sector_count = validate_read_buffer(buf)?;

        let buf_phys = kernel_virt_to_phys(buf.as_ptr() as usize)
            .ok_or(BlockError::IoError)?;

        *self.desc_table = Adma2Desc::new(buf_phys as u64, buf.len() as u16, true);

        flush_dcache_for_dma();

        write32(REG_ADMA_ADDR_LOW, self.desc_table_phys as u32);
        write32(REG_ADMA_ADDR_HIGH, (self.desc_table_phys >> 32) as u32);

        write16(REG_NORMAL_INT_STATUS, 0xffff);
        write16(REG_ERROR_INT_STATUS, 0xffff);

        write16(REG_BLOCK_SIZE, BLOCK_SIZE as u16);
        write16(REG_BLOCK_COUNT, sector_count as u16);
        write8(REG_DATA_TIMEOUT, DATA_TIMEOUT_VALUE);

        write32(REG_ARGUMENT, sector);

        write16(REG_NORMAL_INT_SIGNAL_EN, INT_TRANSFER_COMPLETE);
        write16(REG_ERROR_INT_SIGNAL_EN, 0xffff);

        let (xfer_mode, cmd_index) = if sector_count == 1 {
            (xfer_dir | XFER_MODE_DMA_ENABLE, cmd_single)
        } else {
            (xfer_dir | XFER_MODE_DMA_ENABLE | XFER_MODE_BLOCK_COUNT_EN
                | XFER_MODE_MULTI_BLOCK | XFER_MODE_AUTO_CMD12_EN,
             cmd_multi)
        };

        let cmd: u16 = (cmd_index << CMD_INDEX_SHIFT)
            | CMD_DATA_PRESENT
            | CMD_CRC_CHECK
            | CMD_INDEX_CHECK
            | CMD_RESP_TYPE_R1;

        write32(REG_XFER_MODE, (xfer_mode as u32) | ((cmd as u32) << 16));

        Ok(())
    }
}

impl BlockDriver for SdCardAdma {
    fn name(&self) -> &'static str {
        "sd0"
    }

    fn set_completion_handler(&self, handler: fn(Result<(), BlockError>)) {
        COMPLETION_HANDLER.store(handler as *mut (), Ordering::Relaxed);
    }

    fn start_read(&mut self, sector: u32, buf: &mut [u8]) -> Result<(), BlockError> {
        self.start_io(sector, buf, XFER_MODE_DATA_DIR_READ, SD_CMD17_READ_SINGLE, SD_CMD18_READ_MULTIPLE)
    }

    fn start_write(&mut self, sector: u32, buf: &[u8]) -> Result<(), BlockError> {
        self.start_io(sector, buf, 0, SD_CMD24_WRITE_SINGLE, SD_CMD25_WRITE_MULTIPLE)
    }
}

/// SD interrupt handler
fn sd_irq_handler(_irq: u32) {
    let status = read16(REG_NORMAL_INT_STATUS);
    let err = read16(REG_ERROR_INT_STATUS);

    // Disable signal generation first
    write16(REG_NORMAL_INT_SIGNAL_EN, 0);
    write16(REG_ERROR_INT_SIGNAL_EN, 0);

    // Check for errors
    if err != 0 {
        write16(REG_ERROR_INT_STATUS, 0xffff);
        call_completion_handler(Err(BlockError::IoError));
        return;
    }

    // Check for Transfer Complete
    if status & INT_TRANSFER_COMPLETE != 0 {
        write16(REG_NORMAL_INT_STATUS, INT_TRANSFER_COMPLETE);

        // Invalidate cache so CPU sees DMA-written data
        flush_dcache_for_dma();

        call_completion_handler(Ok(()));
    }

    // Clear any other status bits
    if status & !INT_TRANSFER_COMPLETE != 0 {
        write16(REG_NORMAL_INT_STATUS, status & !INT_TRANSFER_COMPLETE);
    }
}

/// Call the registered completion handler, if any.
fn call_completion_handler(result: Result<(), BlockError>) {
    let handler_ptr = COMPLETION_HANDLER.load(Ordering::Relaxed);
    if !handler_ptr.is_null() {
        // SAFETY: handler_ptr was stored by set_completion_handler via `handler as *mut ()`.
        // The value is a valid fn(Result<(), BlockError>) pointer; transmute recovers it.
        let handler: fn(Result<(), BlockError>) = unsafe { transmute(handler_ptr) };
        handler(result);
    }
}

/// Flush data cache for DMA coherency on T-Head C906
#[inline]
fn flush_dcache_for_dma() {
    unsafe {
        if dtb::get_cpu_type() == dtb::CpuType::LicheeRVNano {
            core::arch::asm!(
                ".long 0x0030000b",   // th.dcache.ciall
                options(nostack, preserves_flags),
            );
        }
        core::arch::asm!("fence", options(nostack, preserves_flags));
    }
}

// Register access functions
fn read16(offset: usize) -> u16 {
    unsafe { ((SD_BASE + offset) as *const u16).read_volatile() }
}

fn read8(offset: usize) -> u8 {
    unsafe { ((SD_BASE + offset) as *const u8).read_volatile() }
}

fn write32(offset: usize, val: u32) {
    unsafe { ((SD_BASE + offset) as *mut u32).write_volatile(val) }
}

fn write16(offset: usize, val: u16) {
    unsafe { ((SD_BASE + offset) as *mut u16).write_volatile(val) }
}

fn write8(offset: usize, val: u8) {
    unsafe { ((SD_BASE + offset) as *mut u8).write_volatile(val) }
}
