//! Demo: Read SD card controller (SDHCI) registers on the NanoRV.
//!
//! The CV181x SD controller at 0x0431_0000 is SDHCI-compatible.
//! This demo reads standard SDHCI registers to verify the controller
//! is accessible and check whether a card is present.

const SD_BASE: usize = 0x0431_0000;

// SDHCI register offsets
const REG_ARGUMENT: usize = 0x08;
const REG_XFER_MODE: usize = 0x0C;
const REG_COMMAND: usize = 0x0E;
const REG_RESPONSE0: usize = 0x10;
const REG_RESPONSE2: usize = 0x14;
const REG_RESPONSE4: usize = 0x18;
const REG_RESPONSE6: usize = 0x1C;
const REG_PRESENT_STATE: usize = 0x24;
const REG_BLOCK_SIZE: usize = 0x04;
const REG_BLOCK_COUNT: usize = 0x06;
const REG_BUFFER_DATA: usize = 0x20;
const REG_NORMAL_INT_STATUS: usize = 0x30;
const REG_ERROR_INT_STATUS: usize = 0x32;
const REG_NORMAL_INT_STATUS_EN: usize = 0x34;
const REG_ERROR_INT_STATUS_EN: usize = 0x36;

// Present State bits
const PRESENT_CMD_INHIBIT: u32 = 1 << 0;
const PRESENT_DAT_INHIBIT: u32 = 1 << 1;

// Normal Interrupt Status bits
const INT_COMMAND_COMPLETE: u16 = 1 << 0;
const INT_TRANSFER_COMPLETE: u16 = 1 << 1;
const INT_BUFFER_READ_READY: u16 = 1 << 5;

// Error Interrupt Status bits
const ERR_CMD_TIMEOUT: u16 = 1 << 0;
const ERR_CMD_CRC: u16 = 1 << 1;
const ERR_CMD_END_BIT: u16 = 1 << 2;
const ERR_CMD_INDEX: u16 = 1 << 3;

/// SDHCI response types, which determine command register flags.
#[derive(Copy, Clone)]
enum ResponseType {
    /// No response (e.g. CMD0)
    None,
    /// 48-bit response with CRC and index check (R1, R6, R7)
    R1,
    /// 48-bit response with busy signal (R1b)
    R1b,
    /// 136-bit response with CRC check (R2)
    R2,
    /// 48-bit response, no CRC/index check (R3 — OCR)
    R3,
}

#[derive(Debug)]
enum SdError {
    CommandTimeout,
    CommandCrc,
    CommandEndBit,
    CommandIndex,
    UnknownError(u16),
    PollTimeout,
}

impl core::fmt::Display for SdError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            SdError::CommandTimeout => write!(f, "command timeout"),
            SdError::CommandCrc => write!(f, "command CRC error"),
            SdError::CommandEndBit => write!(f, "command end bit error"),
            SdError::CommandIndex => write!(f, "command index error"),
            SdError::UnknownError(e) => write!(f, "unknown error {:#06x}", e),
            SdError::PollTimeout => write!(f, "poll timeout (controller not responding)"),
        }
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

fn read8(offset: usize) -> u8 {
    let addr = (SD_BASE + offset) as *const u8;
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

// =============================================================================
// Command engine
// =============================================================================

/// Send an SD command via the SDHCI controller and return the response.
///
/// Handles the full SDHCI command flow:
/// 1. Wait for Command Inhibit to clear
/// 2. Clear stale interrupt status
/// 3. Write argument and command registers
/// 4. Poll for command completion
/// 5. Check for errors and read response
///
/// Returns up to 4 response words (only [0] is meaningful for 48-bit responses;
/// all 4 are used for 136-bit R2 responses).
fn send_command(cmd_index: u16, argument: u32, resp: ResponseType) -> Result<[u32; 4], SdError> {
    // Wait for Command Inhibit (CMD) to clear
    let mut timeout = 100_000u32;
    while read32(REG_PRESENT_STATE) & PRESENT_CMD_INHIBIT != 0 {
        timeout -= 1;
        if timeout == 0 {
            return Err(SdError::PollTimeout);
        }
    }

    // For commands that use the DAT line (R1b has busy signal on DAT0),
    // also wait for DAT inhibit to clear
    if matches!(resp, ResponseType::R1b) {
        timeout = 100_000;
        while read32(REG_PRESENT_STATE) & PRESENT_DAT_INHIBIT != 0 {
            timeout -= 1;
            if timeout == 0 {
                return Err(SdError::PollTimeout);
            }
        }
    }

    // Clear any pending interrupt status
    write16(REG_NORMAL_INT_STATUS, 0xFFFF);
    write16(REG_ERROR_INT_STATUS, 0xFFFF);

    // Write argument
    write32(REG_ARGUMENT, argument);

    // Build command register value:
    //   [13:8] = command index
    //   [7:6]  = command type (00 = normal)
    //   [5]    = data present (0 for now — data reads use a separate path)
    //   [4]    = command index check enable
    //   [3]    = command CRC check enable
    //   [1:0]  = response type select
    let cmd_flags: u16 = match resp {
        ResponseType::None => 0x00,
        ResponseType::R1   => 0x1A, // 48-bit, CRC check, index check
        ResponseType::R1b  => 0x1B, // 48-bit+busy, CRC check, index check
        ResponseType::R2   => 0x09, // 136-bit, CRC check
        ResponseType::R3   => 0x02, // 48-bit, no CRC/index check
    };
    let cmd_reg = (cmd_index << 8) | cmd_flags;

    // Write command register — this triggers the command on the bus
    write16(REG_COMMAND, cmd_reg);

    // For commands with no response, we're done
    if matches!(resp, ResponseType::None) {
        // Brief spin to let the card process the command
        for _ in 0..10_000u32 {
            core::hint::spin_loop();
        }
        return Ok([0; 4]);
    }

    // Poll for Command Complete
    timeout = 1_000_000;
    loop {
        let status = read16(REG_NORMAL_INT_STATUS);
        if status & INT_COMMAND_COMPLETE != 0 {
            // Clear Command Complete
            write16(REG_NORMAL_INT_STATUS, INT_COMMAND_COMPLETE);
            break;
        }
        timeout -= 1;
        if timeout == 0 {
            // Check if it was actually an error
            let err = read16(REG_ERROR_INT_STATUS);
            if err & ERR_CMD_TIMEOUT != 0 {
                write16(REG_ERROR_INT_STATUS, 0xFFFF);
                return Err(SdError::CommandTimeout);
            }
            return Err(SdError::PollTimeout);
        }
    }

    // Check for errors
    let err = read16(REG_ERROR_INT_STATUS);
    if err != 0 {
        write16(REG_ERROR_INT_STATUS, 0xFFFF);
        if err & ERR_CMD_TIMEOUT != 0 { return Err(SdError::CommandTimeout); }
        if err & ERR_CMD_CRC != 0 { return Err(SdError::CommandCrc); }
        if err & ERR_CMD_END_BIT != 0 { return Err(SdError::CommandEndBit); }
        if err & ERR_CMD_INDEX != 0 { return Err(SdError::CommandIndex); }
        return Err(SdError::UnknownError(err));
    }

    // Read response
    let response = [
        read32(REG_RESPONSE0),
        read32(REG_RESPONSE2),
        read32(REG_RESPONSE4),
        read32(REG_RESPONSE6),
    ];

    Ok(response)
}

// =============================================================================
// Card type detection
// =============================================================================

/// Detect the SD card type by re-running the identification sequence.
///
/// Sends CMD0 (reset), CMD8 (voltage check), and ACMD41 (OCR read)
/// to determine whether the card is SDHC/SDXC (block-addressed) or
/// SDSC (byte-addressed).
fn detect_card_type() {
    println!("--- Card Type Detection ---");
    println!();

    // Enable all normal and error interrupt status bits for polling
    write16(REG_NORMAL_INT_STATUS_EN, 0xFFFF);
    write16(REG_ERROR_INT_STATUS_EN, 0xFFFF);

    // CMD0: GO_IDLE_STATE — reset card to idle
    println!("CMD0 (GO_IDLE_STATE)...");
    match send_command(0, 0, ResponseType::None) {
        Ok(_) => println!("  OK (no response expected)"),
        Err(e) => {
            println!("  Error: {}", e);
            return;
        }
    }

    // CMD8: SEND_IF_COND — verify voltage, detect SD v2.0+
    // Argument: [11:8] = voltage (1 = 2.7-3.6V), [7:0] = check pattern (0xAA)
    println!("CMD8 (SEND_IF_COND, arg=0x1AA)...");
    let sd_v2 = match send_command(8, 0x1AA, ResponseType::R1) {
        Ok(resp) => {
            let echo = resp[0] & 0x1FF;
            println!("  Response: {:#010x}", resp[0]);
            println!("  Voltage accepted: {}", if (resp[0] >> 8) & 0xF == 1 { "2.7-3.6V" } else { "?" });
            println!("  Check pattern: {:#x} ({})", echo & 0xFF,
                if echo == 0x1AA { "match" } else { "MISMATCH" });
            if echo == 0x1AA {
                println!("  -> SD v2.0+ (SDHC possible)");
                true
            } else {
                println!("  -> Check pattern mismatch, unexpected");
                false
            }
        }
        Err(SdError::CommandTimeout) => {
            println!("  Timeout — SD v1.x card (SDSC only)");
            false
        }
        Err(e) => {
            println!("  Error: {}", e);
            return;
        }
    };

    // ACMD41: SD_SEND_OP_COND — read OCR, negotiate voltage, check SDHC
    // Must be preceded by CMD55 (APP_CMD) each time.
    // Argument: HCS (bit 30) = 1 if SD v2.0+, voltage window bits [23:0]
    let acmd41_arg: u32 = if sd_v2 { 0x40FF8000 } else { 0x00FF8000 };
    println!("ACMD41 (SD_SEND_OP_COND, HCS={})...", if sd_v2 { 1 } else { 0 });

    let mut attempts = 0u32;
    let max_attempts = 100;

    let ocr = loop {
        // CMD55: APP_CMD — next command is an application command
        // RCA = 0 (card not yet identified)
        match send_command(55, 0, ResponseType::R1) {
            Ok(_) => {}
            Err(e) => {
                println!("  CMD55 error: {}", e);
                return;
            }
        }

        // ACMD41: SD_SEND_OP_COND
        match send_command(41, acmd41_arg, ResponseType::R3) {
            Ok(resp) => {
                let ocr = resp[0];
                attempts += 1;
                if ocr & (1 << 31) != 0 {
                    // Card is ready (busy bit set = NOT busy)
                    break ocr;
                }
                if attempts >= max_attempts {
                    println!("  Card not ready after {} attempts (OCR: {:#010x})", attempts, ocr);
                    return;
                }
                // Brief delay before retry
                for _ in 0..50_000u32 {
                    core::hint::spin_loop();
                }
            }
            Err(e) => {
                println!("  ACMD41 error: {}", e);
                return;
            }
        }
    };

    println!("  Card ready after {} attempt(s)", attempts);
    println!("  OCR: {:#010x}", ocr);
    println!("  Voltage: 3.2-3.3V={} 3.3-3.4V={}", (ocr >> 20) & 1, (ocr >> 21) & 1);

    let ccs = (ocr >> 30) & 1;
    if sd_v2 {
        println!("  CCS (Card Capacity Status): {}", ccs);
        if ccs == 1 {
            println!("  -> SDHC/SDXC — block addressing (sector = 512 bytes)");
        } else {
            println!("  -> SDSC — byte addressing");
        }
    } else {
        println!("  -> SDSC (v1.x card) — byte addressing");
    }

    println!();
    select_card();
}

/// Move the card from 'ready' state to 'transfer' state.
///
/// After identification (CMD0/CMD8/ACMD41), the card is in 'ready' state.
/// We need:
///   CMD2 (ALL_SEND_CID)    — card publishes its CID, moves to 'ident' state
///   CMD3 (SEND_RELATIVE_ADDR) — card publishes its RCA, moves to 'stby' state
///   CMD7 (SELECT_CARD)     — select card by RCA, moves to 'tran' state
fn select_card() {
    println!("--- Select Card ---");
    println!();

    // CMD2: ALL_SEND_CID — get the Card Identification register (136-bit R2 response)
    println!("CMD2 (ALL_SEND_CID)...");
    match send_command(2, 0, ResponseType::R2) {
        Ok(resp) => {
            // R2 response: 128 bits of CID spread across resp[0..4]
            // SDHCI shifts the R2 response left by 8 bits and drops the CRC,
            // so the 128-bit payload is in bits [127:0] across the 4 words.
            println!("  CID: {:08x} {:08x} {:08x} {:08x}", resp[3], resp[2], resp[1], resp[0]);

            // Parse CID fields (SD card format):
            //   [127:120] MID (manufacturer ID)     — resp[3] bits [31:24]
            //   [119:104] OID (OEM/application ID)   — resp[3] bits [23:8]
            //   [103:64]  PNM (product name, 5 chars) — resp[3][7:0], resp[2], resp[1][31:24]
            //   [63:56]   PRV (product revision)     — resp[1] bits [23:16]
            //   [55:24]   PSN (serial number)        — resp[1][15:0], resp[0][31:16]
            let mid = (resp[3] >> 24) & 0xFF;
            let oid_hi = ((resp[3] >> 8) & 0xFF) as u8 as char;
            let oid_lo = (resp[3] & 0xFF) as u8 as char;
            let pnm: [u8; 5] = [
                ((resp[2] >> 24) & 0xFF) as u8,
                ((resp[2] >> 16) & 0xFF) as u8,
                ((resp[2] >> 8) & 0xFF) as u8,
                (resp[2] & 0xFF) as u8,
                ((resp[1] >> 24) & 0xFF) as u8,
            ];
            let prv = (resp[1] >> 16) & 0xFF;
            let psn = ((resp[1] & 0xFFFF) << 16) | ((resp[0] >> 16) & 0xFFFF);

            println!("  Manufacturer ID:  {:#04x}", mid);
            println!("  OEM ID:           {}{}", oid_hi, oid_lo);
            print!("  Product Name:     ");
            for &b in &pnm {
                if b >= 0x20 && b < 0x7F { print!("{}", b as char); } else { print!("?"); }
            }
            println!();
            println!("  Product Revision: {}.{}", (prv >> 4) & 0xF, prv & 0xF);
            println!("  Serial Number:    {:#010x}", psn);
        }
        Err(e) => {
            println!("  Error: {}", e);
            return;
        }
    }

    // CMD3: SEND_RELATIVE_ADDR — card publishes its RCA
    println!("CMD3 (SEND_RELATIVE_ADDR)...");
    let rca: u32 = match send_command(3, 0, ResponseType::R1) {
        Ok(resp) => {
            // R6 response: [31:16] = new RCA, [15:0] = card status bits
            let rca = (resp[0] >> 16) & 0xFFFF;
            let status = resp[0] & 0xFFFF;
            println!("  RCA: {:#06x}", rca);
            println!("  Status: {:#06x}", status);
            rca
        }
        Err(e) => {
            println!("  Error: {}", e);
            return;
        }
    };

    // CMD7: SELECT/DESELECT_CARD — select the card by RCA, enters 'tran' state
    println!("CMD7 (SELECT_CARD, RCA={:#06x})...", rca);
    match send_command(7, rca << 16, ResponseType::R1b) {
        Ok(resp) => {
            let status = resp[0];
            let state = (status >> 9) & 0xF;
            println!("  Card status: {:#010x}", status);
            println!("  Card state:  {} ({})", state, match state {
                0 => "idle", 1 => "ready", 2 => "ident", 3 => "stby",
                4 => "tran", 5 => "data", 6 => "rcv", 7 => "prg",
                8 => "dis", _ => "?",
            });
            // CMD7 response shows the state *before* the command takes effect.
            // State 3 (stby) is expected — the card transitions to tran after processing CMD7.
            if state == 3 || state == 4 {
                println!("  -> Card selected, now in transfer state");
            } else {
                println!("  -> WARNING: unexpected state {}", state);
            }
        }
        Err(e) => {
            println!("  Error: {}", e);
            return;
        }
    }

    // Switch card and controller to 4-bit bus mode.
    // CMD0 reset the card to 1-bit; U-Boot left the controller in 4-bit.
    // ACMD6 = CMD55 (with RCA) + CMD6 (arg=2 for 4-bit)
    println!("ACMD6 (SET_BUS_WIDTH, 4-bit)...");
    match send_command(55, rca << 16, ResponseType::R1) {
        Ok(_) => {}
        Err(e) => {
            println!("  CMD55 error: {}", e);
            return;
        }
    }
    match send_command(6, 2, ResponseType::R1) {
        Ok(_) => {
            // Set controller Host Control 1 bit 1 = 1 for 4-bit mode
            let hc = read8(0x28);
            write8(0x28, hc | 0x02);
            println!("  OK (card and controller now in 4-bit mode)");
        }
        Err(e) => {
            println!("  Error: {}", e);
            return;
        }
    }

    println!();
    read_sector_zero();
}

// =============================================================================
// Block read
// =============================================================================

/// Read a single 512-byte block from the SD card.
///
/// Implements plan steps 4-8: set up block size/count, issue CMD17 with
/// data-present flag, wait for command complete, wait for buffer read ready,
/// read 128 words from the data port, and wait for transfer complete.
fn read_block(sector: u32) -> Result<[u8; 512], SdError> {
    // Wait for both CMD and DAT inhibit to clear
    let mut timeout = 100_000u32;
    while read32(REG_PRESENT_STATE) & (PRESENT_CMD_INHIBIT | PRESENT_DAT_INHIBIT) != 0 {
        timeout -= 1;
        if timeout == 0 {
            println!("  read_block: inhibit timeout, present={:#010x}", read32(REG_PRESENT_STATE));
            return Err(SdError::PollTimeout);
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

    // Transfer Mode + Command as a single 32-bit write at 0x0C.
    // The CV181x controller requires this atomic write to latch Transfer Mode.
    //
    // Transfer Mode (lower 16 bits): 0x0010 — single block, read, no DMA
    // Command (upper 16 bits): 0x113A — CMD17, R1, data present, CRC+index check
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
            if err & ERR_CMD_TIMEOUT != 0 { return Err(SdError::CommandTimeout); }
            if err & ERR_CMD_CRC != 0 { return Err(SdError::CommandCrc); }
            if err & ERR_CMD_END_BIT != 0 { return Err(SdError::CommandEndBit); }
            if err & ERR_CMD_INDEX != 0 { return Err(SdError::CommandIndex); }
            return Err(SdError::UnknownError(err));
        }
        timeout -= 1;
        if timeout == 0 {
            println!("  read_block: cmd complete timeout, normal={:#06x} error={:#06x} present={:#010x}",
                status, read16(REG_ERROR_INT_STATUS), read32(REG_PRESENT_STATE));
            return Err(SdError::PollTimeout);
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
            return Err(SdError::UnknownError(err));
        }
        timeout -= 1;
        if timeout == 0 {
            println!("  read_block: buffer ready timeout, normal={:#06x} error={:#06x} present={:#010x}",
                status, read16(REG_ERROR_INT_STATUS), read32(REG_PRESENT_STATE));
            return Err(SdError::PollTimeout);
        }
    }

    // Read 512 bytes (128 x 32-bit words) from Buffer Data Port
    let mut buf = [0u8; 512];
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
            println!("  read_block: xfer complete timeout, normal={:#06x} error={:#06x}",
                read16(REG_NORMAL_INT_STATUS), err);
            write16(REG_ERROR_INT_STATUS, 0xFFFF);
            if err != 0 { return Err(SdError::UnknownError(err)); }
            return Err(SdError::PollTimeout);
        }
    }

    Ok(buf)
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

// =============================================================================
// Public demo entry points
// =============================================================================

pub fn sd_read_demo() {
    println!("=== SD Controller Register Dump (SDHCI @ {:#x}) ===", SD_BASE);
    println!();

    // Host Controller Version (offset 0xFE, 16-bit)
    let version = read16(0xFE);
    let spec_ver = version & 0xFF;
    let vendor_ver = (version >> 8) & 0xFF;
    println!("Host Controller Version: {:#06x}", version);
    println!("  Spec version: {} ({})", spec_ver, match spec_ver {
        0 => "SDHCI 1.0",
        1 => "SDHCI 2.0",
        2 => "SDHCI 3.0",
        3 => "SDHCI 4.0",
        4 => "SDHCI 4.1",
        5 => "SDHCI 4.2",
        _ => "unknown",
    });
    println!("  Vendor version: {:#x}", vendor_ver);
    println!();

    // Present State (offset 0x24, 32-bit)
    let present = read32(REG_PRESENT_STATE);
    println!("Present State: {:#010x}", present);
    println!("  Command Inhibit (CMD):  {}", (present >> 0) & 1);
    println!("  Command Inhibit (DAT):  {}", (present >> 1) & 1);
    println!("  DAT Line Active:        {}", (present >> 2) & 1);
    println!("  Write Transfer Active:  {}", (present >> 8) & 1);
    println!("  Read Transfer Active:   {}", (present >> 9) & 1);
    println!("  Buffer Write Enable:    {}", (present >> 10) & 1);
    println!("  Buffer Read Enable:     {}", (present >> 11) & 1);
    println!("  Card Inserted:          {}", (present >> 16) & 1);
    println!("  Card Stable:            {}", (present >> 17) & 1);
    println!("  Card Detect Pin Level:  {}", (present >> 18) & 1);
    println!("  Write Protect Pin:      {}", (present >> 19) & 1);
    println!("  DAT[3:0] Line Level:    {:#x}", (present >> 20) & 0xF);
    println!("  CMD Line Level:         {}", (present >> 24) & 1);
    println!();

    // Capabilities (offset 0x40, 64-bit as two 32-bit reads)
    let caps_lo = read32(0x40);
    let caps_hi = read32(0x44);
    println!("Capabilities: {:#010x}_{:08x}", caps_hi, caps_lo);
    println!("  Timeout Clock Freq:     {} {}", caps_lo & 0x3F,
        if (caps_lo >> 7) & 1 == 1 { "MHz" } else { "KHz" });
    println!("  Base Clock Freq:        {} MHz", (caps_lo >> 8) & 0xFF);
    println!("  Max Block Length:        {} bytes", match (caps_lo >> 16) & 0x3 {
        0 => 512, 1 => 1024, 2 => 2048, _ => 0,
    });
    println!("  8-bit support:          {}", (caps_lo >> 18) & 1);
    println!("  ADMA2 support:          {}", (caps_lo >> 19) & 1);
    println!("  High Speed support:     {}", (caps_lo >> 21) & 1);
    println!("  SDMA support:           {}", (caps_lo >> 22) & 1);
    println!("  Suspend/Resume:         {}", (caps_lo >> 23) & 1);
    println!("  3.3V support:           {}", (caps_lo >> 24) & 1);
    println!("  3.0V support:           {}", (caps_lo >> 25) & 1);
    println!("  1.8V support:           {}", (caps_lo >> 26) & 1);
    println!("  64-bit System Bus:      {}", (caps_lo >> 28) & 1);
    println!("  Async Interrupt:        {}", (caps_lo >> 29) & 1);
    println!();

    // Power Control (offset 0x29, 8-bit)
    let power = read8(0x29);
    println!("Power Control: {:#04x}", power);
    println!("  SD Bus Power:           {}", power & 1);
    println!("  Voltage Select:         {}", match (power >> 1) & 0x7 {
        0x7 => "3.3V",
        0x6 => "3.0V",
        0x5 => "1.8V",
        _ => "off/unknown",
    });
    println!();

    // Clock Control (offset 0x2C, 16-bit)
    let clock = read16(0x2C);
    println!("Clock Control: {:#06x}", clock);
    println!("  Internal Clock Enable:  {}", (clock >> 0) & 1);
    println!("  Internal Clock Stable:  {}", (clock >> 1) & 1);
    println!("  SD Clock Enable:        {}", (clock >> 2) & 1);
    let sdclk_div = ((clock >> 8) & 0xFF) as u32;
    println!("  Clock Divider:          {} (divisor={})", sdclk_div,
        if sdclk_div == 0 { 1 } else { sdclk_div * 2 });
    println!();

    // Normal/Error Interrupt Status (offset 0x30, 32-bit)
    let int_status = read32(REG_NORMAL_INT_STATUS);
    println!("Interrupt Status: {:#010x}", int_status);
    println!("  Normal: {:#06x}  Error: {:#06x}", int_status & 0xFFFF, (int_status >> 16) & 0xFFFF);
    println!();

    // Host Control 1 (offset 0x28, 8-bit)
    let host_ctrl = read8(0x28);
    println!("Host Control 1: {:#04x}", host_ctrl);
    println!("  LED Control:            {}", host_ctrl & 1);
    println!("  Data Transfer Width:    {}", if (host_ctrl >> 1) & 1 == 1 { "4-bit" } else { "1-bit" });
    println!("  High Speed Enable:      {}", (host_ctrl >> 2) & 1);
    println!("  DMA Select:             {}", match (host_ctrl >> 3) & 0x3 {
        0 => "SDMA", 1 => "ADMA1", 2 => "ADMA2", 3 => "ADMA2(64)", _ => "?",
    });
    println!();

    // CVI vendor-specific register area (offset 0x200)
    let vendor_0 = read32(0x200);
    let vendor_4 = read32(0x204);
    let vendor_8 = read32(0x208);
    println!("Vendor Registers (offset 0x200):");
    println!("  [0x200]: {:#010x}", vendor_0);
    println!("  [0x204]: {:#010x}", vendor_4);
    println!("  [0x208]: {:#010x}", vendor_8);
    println!();

    // Card type detection
    detect_card_type();
}
