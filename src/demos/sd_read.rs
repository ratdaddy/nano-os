//! SD card driver for NanoRV (SDHCI-compatible CV181x controller)
//!
//! Structured as a device driver with initialization and I/O operations.

use super::block_device::{BlockDevice, BlockError};
use super::disk_inspect;

const SD_BASE: usize = 0x0431_0000;

// SDHCI register offsets
const REG_ARGUMENT: usize = 0x08;
const REG_XFER_MODE: usize = 0x0C;
const REG_PRESENT_STATE: usize = 0x24;
const REG_BLOCK_SIZE: usize = 0x04;
const REG_BLOCK_COUNT: usize = 0x06;
const REG_BUFFER_DATA: usize = 0x20;
const REG_NORMAL_INT_STATUS: usize = 0x30;
const REG_ERROR_INT_STATUS: usize = 0x32;
const REG_NORMAL_INT_STATUS_EN: usize = 0x34;
const REG_ERROR_INT_STATUS_EN: usize = 0x36;
const REG_CAPABILITIES: usize = 0x40;
const REG_CAPABILITIES_HI: usize = 0x44;

// Present State bits
const PRESENT_CMD_INHIBIT: u32 = 1 << 0;
const PRESENT_DAT_INHIBIT: u32 = 1 << 1;

// Normal Interrupt Status bits
const INT_COMMAND_COMPLETE: u16 = 1 << 0;
const INT_TRANSFER_COMPLETE: u16 = 1 << 1;
const INT_BUFFER_READ_READY: u16 = 1 << 5;

// Error Interrupt Status bits
const ERR_CMD_TIMEOUT: u16 = 1 << 0;

/// SD card driver
pub struct SdCard;

impl SdCard {
    /// Create SD card driver instance
    ///
    /// Assumes the card is already initialized by the bootloader (U-Boot).
    /// We just booted from this card, so it's already in transfer state.
    pub fn new() -> Result<Self, BlockError> {
        // Enable interrupt status bits for polling during reads
        write16(REG_NORMAL_INT_STATUS_EN, 0xFFFF);
        write16(REG_ERROR_INT_STATUS_EN, 0xFFFF);

        // Card is already initialized by bootloader - no setup needed
        Ok(SdCard)
    }
}

impl BlockDevice for SdCard {
    fn read_block(&mut self, sector: u32, buf: &mut [u8; 512]) -> Result<(), BlockError> {
        // Wait for both CMD and DAT inhibit to clear
        let mut timeout = 100_000u32;
        while read32(REG_PRESENT_STATE) & (PRESENT_CMD_INHIBIT | PRESENT_DAT_INHIBIT) != 0 {
            timeout -= 1;
            if timeout == 0 {
                return Err(BlockError::Timeout);
            }
        }

        // Clear any pending interrupt status
        write16(REG_NORMAL_INT_STATUS, 0xFFFF);
        write16(REG_ERROR_INT_STATUS, 0xFFFF);

        // Ensure status enable bits are set for polling
        write16(REG_NORMAL_INT_STATUS_EN, 0xFFFF);
        write16(REG_ERROR_INT_STATUS_EN, 0xFFFF);

        // Set block size = 512, block count = 1, data timeout to maximum
        write16(REG_BLOCK_SIZE, 0x0200);
        write16(REG_BLOCK_COUNT, 0x0001);
        write8(0x2E, 0x0E);

        // Write argument (sector number for SDHC)
        write32(REG_ARGUMENT, sector);

        // Transfer Mode + Command as a single 32-bit write
        write32(REG_XFER_MODE, 0x0010 | (0x113A << 16));

        // Wait for Command Complete
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
            if timeout == 0 {
                return Err(BlockError::Timeout);
            }
        }

        // Wait for Buffer Read Ready
        timeout = 250_000;
        loop {
            let status = read16(REG_NORMAL_INT_STATUS);
            if status & INT_BUFFER_READ_READY != 0 {
                write16(REG_NORMAL_INT_STATUS, INT_BUFFER_READ_READY);
                break;
            }
            let err = read16(REG_ERROR_INT_STATUS);
            if err != 0 {
                write16(REG_ERROR_INT_STATUS, 0xFFFF);
                return Err(BlockError::IoError);
            }
            timeout -= 1;
            if timeout == 0 {
                return Err(BlockError::Timeout);
            }
        }

        // Read 512 bytes (128 x 32-bit words) from Buffer Data Port
        for i in 0..128 {
            let word = read32(REG_BUFFER_DATA);
            let offset = i * 4;
            buf[offset] = word as u8;
            buf[offset + 1] = (word >> 8) as u8;
            buf[offset + 2] = (word >> 16) as u8;
            buf[offset + 3] = (word >> 24) as u8;
        }

        // Wait for Transfer Complete
        timeout = 250_000;
        loop {
            let status = read16(REG_NORMAL_INT_STATUS);
            if status & INT_TRANSFER_COMPLETE != 0 {
                write16(REG_NORMAL_INT_STATUS, INT_TRANSFER_COMPLETE);
                break;
            }
            timeout -= 1;
            if timeout == 0 {
                let err = read16(REG_ERROR_INT_STATUS);
                write16(REG_ERROR_INT_STATUS, 0xFFFF);
                if err != 0 { return Err(BlockError::IoError); }
                return Err(BlockError::Timeout);
            }
        }

        Ok(())
    }
}

// =============================================================================
// Register access
// =============================================================================

fn read32(offset: usize) -> u32 {
    let addr = (SD_BASE + offset) as *const u32;
    unsafe { addr.read_volatile() }
}

fn read16(offset: usize) -> u16 {
    let addr = (SD_BASE + offset) as *const u16;
    unsafe { addr.read_volatile() }
}

fn write32(offset: usize, val: u32) {
    let addr = (SD_BASE + offset) as *mut u32;
    unsafe { addr.write_volatile(val) }
}

fn write16(offset: usize, val: u16) {
    let addr = (SD_BASE + offset) as *mut u16;
    unsafe { addr.write_volatile(val) }
}

fn write8(offset: usize, val: u8) {
    let addr = (SD_BASE + offset) as *mut u8;
    unsafe { addr.write_volatile(val) }
}

fn print_capabilities() {
    let cap = read32(REG_CAPABILITIES);
    let cap_hi = read32(REG_CAPABILITIES_HI);

    println!("SDHCI Capabilities:");
    println!("  Lower (0x40): {:#010x}", cap);
    println!("  Upper (0x44): {:#010x}", cap_hi);
    println!();

    // Parse key capability bits from lower register
    println!("  DMA Support:");
    println!("    SDMA:      {}", if cap & (1 << 22) != 0 { "yes" } else { "no" });
    println!("    ADMA1:     {}", if cap & (1 << 18) != 0 { "yes" } else { "no" });
    println!("    ADMA2:     {}", if cap & (1 << 19) != 0 { "yes" } else { "no" });
    println!();

    println!("  Other Features:");
    println!("    64-bit bus: {}", if cap & (1 << 28) != 0 { "yes" } else { "no" });
    println!("    High Speed: {}", if cap & (1 << 21) != 0 { "yes" } else { "no" });
    println!("    3.3V:       {}", if cap & (1 << 24) != 0 { "yes" } else { "no" });
    println!("    3.0V:       {}", if cap & (1 << 25) != 0 { "yes" } else { "no" });
    println!("    1.8V:       {}", if cap & (1 << 26) != 0 { "yes" } else { "no" });

    // Base clock frequency (bits 15-8, in MHz)
    let base_clock = (cap >> 8) & 0xFF;
    println!("    Base clock: {} MHz", base_clock);

    // Max block length (bits 17-16: 0=512, 1=1024, 2=2048, 3=reserved)
    let max_block = match (cap >> 16) & 0x3 {
        0 => 512,
        1 => 1024,
        2 => 2048,
        _ => 0,
    };
    println!("    Max block:  {} bytes", max_block);
    println!();
}

pub fn sd_read_demo() {
    println!("\n=== SD Card Demo ===\n");

    // Print controller capabilities
    print_capabilities();

    // Initialize card
    let mut card = match SdCard::new() {
        Ok(c) => {
            println!("Card initialized successfully");
            println!();
            c
        }
        Err(e) => {
            println!("Failed to initialize card: {}", e);
            return;
        }
    };

    // Inspect disk contents
    disk_inspect::inspect_disk(&mut card);
    println!();
}
