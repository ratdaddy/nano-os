//! SD card driver using ADMA2 (Advanced DMA v2)
//!
//! Fully interrupt-driven implementation for the LicheeRV Nano (SG2002).

use alloc::boxed::Box;

use crate::block::disk;
use crate::drivers::block::validate_read_buffer;
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

/// SD card driver using ADMA2
#[derive(Debug)]
pub struct SdCardAdma {
    desc_table: Box<Adma2Desc>,
    desc_table_phys: usize,
}

impl SdCardAdma {
    fn new() -> Result<Self, BlockError> {
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

        Ok(SdCardAdma { desc_table, desc_table_phys })
    }
}

impl BlockDriver for SdCardAdma {
    fn name(&self) -> &'static str {
        "sd0"
    }

    fn start_read(&mut self, sector: u32, buf: &mut [u8]) -> Result<(), BlockError> {
        // Validate buffer meets DMA requirements
        let sector_count = validate_read_buffer(buf)?;

        // Get physical address of caller's buffer
        let buf_virt = buf.as_ptr() as usize;
        let buf_phys = kernel_virt_to_phys(buf_virt)
            .ok_or(BlockError::IoError)?;

        // Build ADMA2 descriptor for caller's buffer (supports multi-sector)
        *self.desc_table = Adma2Desc::new(buf_phys as u64, buf.len() as u16, true);

        // Flush cache so DMA engine can see the descriptor
        flush_dcache_for_dma();

        // Point ADMA engine to descriptor table
        write32(REG_ADMA_ADDR_LOW, self.desc_table_phys as u32);
        write32(REG_ADMA_ADDR_HIGH, (self.desc_table_phys >> 32) as u32);

        // Clear pending status
        write16(REG_NORMAL_INT_STATUS, 0xffff);
        write16(REG_ERROR_INT_STATUS, 0xffff);

        // Set block size and count
        write16(REG_BLOCK_SIZE, 512);
        write16(REG_BLOCK_COUNT, sector_count as u16);
        write8(REG_DATA_TIMEOUT, DATA_TIMEOUT_VALUE);

        // Write sector argument
        write32(REG_ARGUMENT, sector);

        // Enable interrupt signal for Transfer Complete
        write16(REG_NORMAL_INT_SIGNAL_EN, INT_TRANSFER_COMPLETE);
        write16(REG_ERROR_INT_SIGNAL_EN, 0xffff);

        // Transfer Mode and Command: Different for single vs multi-block
        let (xfer_mode, cmd_index) = if sector_count == 1 {
            // Single block: CMD17, no block count or multi-block flags
            (XFER_MODE_DATA_DIR_READ | XFER_MODE_DMA_ENABLE, SD_CMD17_READ_SINGLE)
        } else {
            // Multi-block: CMD18, with block count, multi-block, and auto-CMD12 to stop
            (XFER_MODE_DATA_DIR_READ | XFER_MODE_DMA_ENABLE
                | XFER_MODE_BLOCK_COUNT_EN | XFER_MODE_MULTI_BLOCK
                | XFER_MODE_AUTO_CMD12_EN,
             SD_CMD18_READ_MULTIPLE)
        };

        // Command: R1 response, data present, CRC + index check
        let cmd: u16 = (cmd_index << CMD_INDEX_SHIFT)
            | CMD_DATA_PRESENT
            | CMD_CRC_CHECK
            | CMD_INDEX_CHECK
            | CMD_RESP_TYPE_R1;

        // Issue command - atomic 32-bit write of transfer mode + command
        write32(REG_XFER_MODE, (xfer_mode as u32) | ((cmd as u32) << 16));

        // Return immediately - interrupt will fire when transfer completes
        Ok(())
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
        disk::send_read_completion(Err(BlockError::IoError));
        return;
    }

    // Check for Transfer Complete
    if status & INT_TRANSFER_COMPLETE != 0 {
        write16(REG_NORMAL_INT_STATUS, INT_TRANSFER_COMPLETE);

        // Invalidate cache so CPU sees DMA-written data
        flush_dcache_for_dma();

        disk::send_read_completion(Ok(()));
    }

    // Clear any other status bits
    if status & !INT_TRANSFER_COMPLETE != 0 {
        write16(REG_NORMAL_INT_STATUS, status & !INT_TRANSFER_COMPLETE);
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

/// Initialize SD ADMA device and register interrupt handler
pub fn init() -> Result<SdCardAdma, BlockError> {

    let device = SdCardAdma::new()?;

    kprintln!("SD ADMA: Registering IRQ {} for device at {:#x}", SD_IRQ, SD_BASE);

    // Register interrupt handler with PLIC
    plic::register_irq(SD_IRQ, sd_irq_handler);

    Ok(device)
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
