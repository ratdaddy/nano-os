//! SD card driver using ADMA2 (Advanced DMA v2)
//!
//! This driver uses SDHCI ADMA2 for DMA transfers instead of PIO.
//! Includes both polling and interrupt-driven read modes.

use super::block_device::{BlockDevice, BlockError};
use super::disk_inspect;
use crate::drivers::plic;
use crate::dtb;
use crate::kernel_memory_map::kernel_virt_to_phys;

const SD_BASE: usize = 0x0431_0000;
const SD_IRQ: u32 = 36;  // From DTB: interrupts = 0x24
const TIMEBASE_FREQUENCY: u64 = 25_000_000;  // From DTB: timebase-frequency = 0x017d7840

// SDHCI register offsets
const REG_BLOCK_SIZE: usize = 0x04;
const REG_BLOCK_COUNT: usize = 0x06;
const REG_ARGUMENT: usize = 0x08;
const REG_XFER_MODE: usize = 0x0C;
const REG_PRESENT_STATE: usize = 0x24;
const REG_HOST_CONTROL: usize = 0x28;
const REG_NORMAL_INT_STATUS: usize = 0x30;
const REG_ERROR_INT_STATUS: usize = 0x32;
const REG_NORMAL_INT_STATUS_EN: usize = 0x34;
const REG_ERROR_INT_STATUS_EN: usize = 0x36;
const REG_NORMAL_INT_SIGNAL_EN: usize = 0x38;
const REG_ERROR_INT_SIGNAL_EN: usize = 0x3A;
const REG_HOST_CONTROL2: usize = 0x3E;
const REG_ADMA_ERROR: usize = 0x54;
const REG_ADMA_ADDR_LOW: usize = 0x58;
const REG_ADMA_ADDR_HIGH: usize = 0x5C;

// Present State bits
const PRESENT_CMD_INHIBIT: u32 = 1 << 0;
const PRESENT_DAT_INHIBIT: u32 = 1 << 1;

// Normal Interrupt Status bits
const INT_COMMAND_COMPLETE: u16 = 1 << 0;
const INT_TRANSFER_COMPLETE: u16 = 1 << 1;

// Error Interrupt Status bits
const ERR_CMD_TIMEOUT: u16 = 1 << 0;

// Host Control register bits (offset 0x28)
const HOST_CTRL_DMA_MASK: u8 = 0x18;
const HOST_CTRL_ADMA2_64: u8 = 0x18;

// Host Control 2 register bits (offset 0x3E)
const HOST_CTRL2_64BIT_ADDR: u16 = 1 << 13;

// ADMA2 descriptor attributes
const ADMA2_VALID: u16 = 1 << 0;
const ADMA2_END: u16 = 1 << 1;
const ADMA2_ACT_TRAN: u16 = 2 << 4;

// RISC-V S-mode interrupt control
const SSTATUS_SIE: usize = 1 << 1;
const SIE_SEIE: usize = 1 << 9;

#[inline]
unsafe fn enable_interrupts() {
    let seie = SIE_SEIE;
    core::arch::asm!("csrs sie, {}", in(reg) seie);
    core::arch::asm!("csrsi sstatus, {}", const SSTATUS_SIE);
}

#[inline]
unsafe fn disable_interrupts() {
    core::arch::asm!("csrci sstatus, {}", const SSTATUS_SIE);
}

#[inline]
unsafe fn wfi() {
    core::arch::asm!("wfi");
}

#[inline]
fn read_time() -> u64 {
    let time: u64;
    unsafe {
        core::arch::asm!("rdtime {}", out(reg) time);
    }
    time
}

/// ADMA2 descriptor (64-bit addressing mode, 128-bit / 16 bytes)
#[repr(C, align(8))]
#[derive(Copy, Clone)]
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

// Static DMA buffers
#[repr(C, align(8))]
struct DescTable([Adma2Desc; 16]);

static mut DESC_TABLE: DescTable = DescTable([Adma2Desc {
    attr: 0, length: 0, addr_lo: 0, addr_hi: 0, _reserved: 0,
}; 16]);

#[repr(C, align(512))]
struct DataBuffer([u8; 512]);

static mut DATA_BUF: DataBuffer = DataBuffer([0; 512]);

// 4KB buffer for multi-block demo
#[repr(C, align(4096))]
struct LargeBuffer([u8; 4096]);

static mut LARGE_BUF: LargeBuffer = LargeBuffer([0; 4096]);

// Interrupt state machine
#[derive(Copy, Clone, PartialEq, Debug)]
enum IrqState {
    Idle,
    WaitingCommandComplete,
    WaitingTransferComplete,
    Complete,
    Error,
}

static mut IRQ_STATE: IrqState = IrqState::Idle;
static mut START_TIME: u64 = 0;
static mut END_TIME: u64 = 0;

/// SD card driver using ADMA2
pub struct SdCardAdma {
    desc_table_phys: usize,
    data_buf_phys: usize,
}

impl SdCardAdma {
    pub fn new() -> Result<Self, BlockError> {
        let desc_table_phys = unsafe {
            kernel_virt_to_phys(core::ptr::addr_of!(DESC_TABLE) as usize)
                .expect("Failed to get physical address for descriptor table")
        };

        let data_buf_phys = unsafe {
            kernel_virt_to_phys(core::ptr::addr_of!(DATA_BUF) as usize)
                .expect("Failed to get physical address for data buffer")
        };

        // Enable interrupt status bits for polling
        write16(REG_NORMAL_INT_STATUS_EN, 0xFFFF);
        write16(REG_ERROR_INT_STATUS_EN, 0xFFFF);

        // Configure HOST_CONTROL2 for 64-bit addressing
        let mut host_ctrl2 = read16(REG_HOST_CONTROL2);
        host_ctrl2 |= HOST_CTRL2_64BIT_ADDR;
        write16(REG_HOST_CONTROL2, host_ctrl2);

        // Configure HOST_CONTROL for ADMA2 64-bit mode
        let mut host_ctrl = read8(REG_HOST_CONTROL);
        host_ctrl = (host_ctrl & !HOST_CTRL_DMA_MASK) | HOST_CTRL_ADMA2_64;
        write8(REG_HOST_CONTROL, host_ctrl);

        Ok(SdCardAdma { desc_table_phys, data_buf_phys })
    }

    /// Set up ADMA descriptor and issue CMD17, returning after command complete.
    /// Caller is responsible for waiting for transfer complete (polling or interrupt).
    unsafe fn issue_read_cmd(&mut self, sector: u32) -> Result<(), BlockError> {
        // Build ADMA2 descriptor
        DESC_TABLE.0[0] = Adma2Desc::new(self.data_buf_phys as u64, 512, true);

        // Flush cache so DMA engine can see the descriptor table
        flush_dcache_for_dma();

        // Point ADMA engine to descriptor table
        write32(REG_ADMA_ADDR_LOW, self.desc_table_phys as u32);
        write32(REG_ADMA_ADDR_HIGH, (self.desc_table_phys >> 32) as u32);

        // Wait for CMD and DAT inhibit to clear
        let mut timeout = 100_000u32;
        while read32(REG_PRESENT_STATE) & (PRESENT_CMD_INHIBIT | PRESENT_DAT_INHIBIT) != 0 {
            timeout -= 1;
            if timeout == 0 { return Err(BlockError::Timeout); }
        }

        // Clear pending status
        write16(REG_NORMAL_INT_STATUS, 0xFFFF);
        write16(REG_ERROR_INT_STATUS, 0xFFFF);

        // Set block size, count, and data timeout
        write16(REG_BLOCK_SIZE, 512);
        write16(REG_BLOCK_COUNT, 1);
        write8(0x2E, 0x0E);

        // Write sector argument
        write32(REG_ARGUMENT, sector);

        // Transfer Mode: DMA enable (bit 0), Read direction (bit 4)
        let xfer_mode: u16 = (1 << 0) | (1 << 4);
        // Command: CMD17, R1 response, data present, CRC + index check
        let cmd: u16 = (17 << 8) | (1 << 5) | (1 << 4) | (1 << 3) | (1 << 1);
        // Atomic 32-bit write of transfer mode + command
        write32(REG_XFER_MODE, (xfer_mode as u32) | ((cmd as u32) << 16));

        // Wait for Command Complete
        timeout = 250_000;
        loop {
            let status = read16(REG_NORMAL_INT_STATUS);
            if status & INT_COMMAND_COMPLETE != 0 {
                write16(REG_NORMAL_INT_STATUS, INT_COMMAND_COMPLETE);
                return Ok(());
            }
            let err = read16(REG_ERROR_INT_STATUS);
            if err != 0 {
                write16(REG_ERROR_INT_STATUS, 0xFFFF);
                if err & ERR_CMD_TIMEOUT != 0 { return Err(BlockError::Timeout); }
                return Err(BlockError::IoError);
            }
            timeout -= 1;
            if timeout == 0 { return Err(BlockError::Timeout); }
        }
    }
}

impl BlockDevice for SdCardAdma {
    fn read_block(&mut self, sector: u32, buf: &mut [u8; 512]) -> Result<(), BlockError> {
        unsafe {
            self.issue_read_cmd(sector)?;

            // Poll for Transfer Complete
            let mut timeout = 250_000u32;
            loop {
                let status = read16(REG_NORMAL_INT_STATUS);
                if status & INT_TRANSFER_COMPLETE != 0 {
                    write16(REG_NORMAL_INT_STATUS, INT_TRANSFER_COMPLETE);
                    break;
                }
                let err = read16(REG_ERROR_INT_STATUS);
                if err != 0 {
                    write16(REG_ERROR_INT_STATUS, 0xFFFF);
                    return Err(BlockError::IoError);
                }
                timeout -= 1;
                if timeout == 0 { return Err(BlockError::Timeout); }
            }

            // Invalidate cache so we see DMA-written data
            flush_dcache_for_dma();
            buf.copy_from_slice(&DATA_BUF.0);
            Ok(())
        }
    }
}

/// SDHCI interrupt handler called by PLIC dispatcher
fn sd_irq_handler(_irq: u32) {
    let status = read16(REG_NORMAL_INT_STATUS);
    let err = read16(REG_ERROR_INT_STATUS);

    unsafe {
        // Check for errors
        if err != 0 {
            write16(REG_ERROR_INT_STATUS, 0xFFFF);
            IRQ_STATE = IrqState::Error;
            return;
        }

        // We only expect Transfer Complete interrupt
        if status & INT_TRANSFER_COMPLETE != 0 {
            write16(REG_NORMAL_INT_STATUS, INT_TRANSFER_COMPLETE);
            IRQ_STATE = IrqState::Complete;
        }

        // Clear any other status bits
        if status & !INT_TRANSFER_COMPLETE != 0 {
            write16(REG_NORMAL_INT_STATUS, status & !INT_TRANSFER_COMPLETE);
        }
    }
}

/// Perform an interrupt-driven ADMA2 read of 1 block (512 bytes).
/// Issues command (polling Command Complete), then waits for Transfer Complete via interrupt.
/// Returns (cycles, microseconds).
fn read_blocks_irq(device: &mut SdCardAdma, start_sector: u32, buf: &mut [u8; 512]) -> Result<(u64, u64), BlockError> {
    unsafe {
        IRQ_STATE = IrqState::WaitingTransferComplete;
        START_TIME = read_time();

        // Build ADMA2 descriptor for 512-byte transfer (single block)
        DESC_TABLE.0[0] = Adma2Desc::new(device.data_buf_phys as u64, 512, true);

        flush_dcache_for_dma();

        // Point ADMA engine to descriptor table
        write32(REG_ADMA_ADDR_LOW, device.desc_table_phys as u32);
        write32(REG_ADMA_ADDR_HIGH, (device.desc_table_phys >> 32) as u32);

        // Wait for CMD and DAT inhibit to clear
        /*
        let mut timeout = 100_000u32;
        while read32(REG_PRESENT_STATE) & (PRESENT_CMD_INHIBIT | PRESENT_DAT_INHIBIT) != 0 {
            timeout -= 1;
            if timeout == 0 { return Err(BlockError::Timeout); }
        }
        */

        // Clear pending status
        write16(REG_NORMAL_INT_STATUS, 0xFFFF);
        write16(REG_ERROR_INT_STATUS, 0xFFFF);

        // Set block size (512) and count (1 block = 512 bytes)
        write16(REG_BLOCK_SIZE, 512);
        write16(REG_BLOCK_COUNT, 1);
        write8(0x2E, 0x0E);

        // Write starting sector argument
        write32(REG_ARGUMENT, start_sector);

        // Issue CMD17 (READ_SINGLE_BLOCK)
        // Transfer Mode: DMA enable (bit 0), Read direction (bit 4)
        let xfer_mode: u16 = (1 << 4) | (1 << 0);
        // Command: CMD17, R1 response, data present, CRC + index check
        let cmd: u16 = (17 << 8) | (1 << 5) | (1 << 4) | (1 << 3) | (1 << 1);
        write32(REG_XFER_MODE, (xfer_mode as u32) | ((cmd as u32) << 16));

        // Wait for Command Complete
        /*
        timeout = 250_000;
        loop {
            let status = read16(REG_NORMAL_INT_STATUS);
            if status & INT_COMMAND_COMPLETE != 0 {
                write16(REG_NORMAL_INT_STATUS, INT_COMMAND_COMPLETE);
                break;
            }
            let err = read16(REG_ERROR_INT_STATUS);
            if err != 0 {
                write16(REG_ERROR_INT_STATUS, 0xFFFF);
                if err & ERR_CMD_TIMEOUT != 0 { return Err(BlockError::Timeout); }
                return Err(BlockError::IoError);
            }
            timeout -= 1;
            if timeout == 0 { return Err(BlockError::Timeout); }
        }
        */

        // Command complete - ADMA transfer is now in progress.
        // Set up interrupt path for Transfer Complete.

        // Set up sscratch for kernel trap handler
        let trap_stack = crate::kernel_trap::trap_stack_top();
        core::arch::asm!("csrw sscratch, {}", in(reg) trap_stack);

        // Enable SDHCI interrupt signal generation for Transfer Complete only
        write16(REG_NORMAL_INT_SIGNAL_EN, INT_TRANSFER_COMPLETE);
        write16(REG_ERROR_INT_SIGNAL_EN, 0xFFFF);

        // Enable CPU interrupts
        enable_interrupts();

        // Check if transfer already completed before we enabled interrupts
        let status = read16(REG_NORMAL_INT_STATUS);
        if status & INT_TRANSFER_COMPLETE != 0 {
            write16(REG_NORMAL_INT_STATUS, INT_TRANSFER_COMPLETE);
            IRQ_STATE = IrqState::Complete;
        }

        // Wait for completion
        let timeout_cycles = TIMEBASE_FREQUENCY;  // 1 second timeout
        loop {
            let state = core::ptr::addr_of!(IRQ_STATE).read_volatile();

            match state {
                IrqState::Complete => {
                    END_TIME = read_time();
                    break;
                }
                IrqState::Error => {
                    disable_interrupts();
                    write16(REG_NORMAL_INT_SIGNAL_EN, 0);
                    write16(REG_ERROR_INT_SIGNAL_EN, 0);
                    return Err(BlockError::IoError);
                }
                _ => {
                    let elapsed = read_time() - START_TIME;
                    if elapsed >= timeout_cycles {
                        disable_interrupts();
                        write16(REG_NORMAL_INT_SIGNAL_EN, 0);
                        write16(REG_ERROR_INT_SIGNAL_EN, 0);
                        return Err(BlockError::Timeout);
                    }
                    //wfi();
                }
            }
        }

        disable_interrupts();

        // Disable signal generation (back to polling mode)
        write16(REG_NORMAL_INT_SIGNAL_EN, 0);
        write16(REG_ERROR_INT_SIGNAL_EN, 0);

        // Calculate elapsed time in microseconds
        let cycles = END_TIME - START_TIME;
        let microseconds = (cycles * 1_000_000) / TIMEBASE_FREQUENCY;

        // Invalidate cache so we see DMA-written data
        flush_dcache_for_dma();
        buf.copy_from_slice(&DATA_BUF.0);

        Ok((cycles, microseconds))
    }
}

/// Flush data cache for DMA coherency.
/// On T-Head C906 (Lichee RV Nano), uses custom cache instructions.
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
fn read32(offset: usize) -> u32 {
    unsafe { ((SD_BASE + offset) as *const u32).read_volatile() }
}

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

pub fn sd_adma_demo() {
    println!("\n=== SD Card ADMA2 Demo ===\n");

    // Initialize driver
    let mut card = match SdCardAdma::new() {
        Ok(c) => c,
        Err(e) => {
            println!("Failed to initialize ADMA2 driver: {}", e);
            return;
        }
    };

    // Test polling read of sector 0 (MBR)
    println!("--- Polling Read ---");
    let mut buf = [0u8; 512];
    match card.read_block(0, &mut buf) {
        Ok(()) => {
            let sig = ((buf[511] as u16) << 8) | (buf[510] as u16);
            println!("MBR signature: {:#06x} {}",
                sig, if sig == 0xAA55 { "(valid)" } else { "(INVALID)" });
        }
        Err(e) => {
            println!("Polling read failed: {}", e);
            return;
        }
    }
    println!();

    // Inspect disk structure
    disk_inspect::inspect_disk(&mut card);
    println!();

    // Interrupt-driven I/O proof of concept
    println!("=== Interrupt-Driven ADMA2 Read ===");
    println!();

    // Register SD IRQ handler with PLIC (combines registration + enabling)
    plic::register_irq(SD_IRQ, sd_irq_handler);

    // Clear any stale SDHCI interrupt status
    write16(REG_NORMAL_INT_STATUS, 0xFFFF);
    write16(REG_ERROR_INT_STATUS, 0xFFFF);

    // Run 10 tests across different areas of the 64GB card
    // Spread tests across 6.4GB intervals
    const SECTOR_STEP: u32 = 13421773;  // ~6.4GB in sectors
    println!("Running 10 interrupt-driven reads (512 bytes each) across 64GB card...");

    let mut buf = [0u8; 512];
    let mut results: [(u32, u64, u64); 10] = [(0, 0, 0); 10];

    for i in 0..10 {
        let sector = i * SECTOR_STEP;
        match read_blocks_irq(&mut card, sector, &mut buf) {
            Ok((cycles, microseconds)) => {
                results[i as usize] = (sector, cycles, microseconds);
            }
            Err(e) => {
                println!("✗ Read {} failed at sector {}: {}", i, sector, e);
                return;
            }
        }
    }

    // Print all results
    println!();
    println!("Results:");
    println!("  Run | Sector      | Offset (GB) | Cycles    | Time (us)");
    println!("  ----|-------------|-------------|-----------|----------");
    for i in 0..10 {
        let (sector, cycles, microseconds) = results[i];
        let gb_offset = (sector as u64 * 512) / (1024 * 1024 * 1024);
        println!("  {:3} | {:11} | {:11} | {:9} | {}", i, sector, gb_offset, cycles, microseconds);
    }
    println!();
}
