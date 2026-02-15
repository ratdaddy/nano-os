//! RISC-V architecture constants for S-mode.
//!
//! These values are defined by the RISC-V Privileged Specification.

#![allow(dead_code)]

// ============================================================================
// sstatus register bits
// ============================================================================

/// sstatus: Supervisor Interrupt Enable
pub const SSTATUS_SIE: usize = 1 << 1;
/// sstatus: Supervisor Previous Interrupt Enable
pub const SSTATUS_SPIE: usize = 1 << 5;
/// sstatus: Supervisor Previous Privilege (0=User, 1=Supervisor)
pub const SSTATUS_SPP: usize = 1 << 8;

// ============================================================================
// sie register bits (Supervisor Interrupt Enable)
// ============================================================================

/// sie: Supervisor Software Interrupt Enable
pub const SIE_SSIE: usize = 1 << 1;
/// sie: Supervisor Timer Interrupt Enable
pub const SIE_STIE: usize = 1 << 5;
/// sie: Supervisor External Interrupt Enable
pub const SIE_SEIE: usize = 1 << 9;
/// sie: Enable all S-mode interrupt sources (software, timer, external)
pub const SIE_ALL: usize = SIE_SSIE | SIE_STIE | SIE_SEIE;

// ============================================================================
// satp register
// ============================================================================

/// satp: Sv39 paging mode (39-bit virtual addresses, 3-level page table)
pub const SATP_MODE_SV39: usize = 8 << 60;

// ============================================================================
// scause values
// ============================================================================

/// Bit 63 of scause indicates an interrupt (1) vs exception (0)
pub const SCAUSE_INTERRUPT_BIT: usize = 1 << 63;

/// S-mode interrupt causes (scause values with interrupt bit set)
pub mod interrupt {
    use super::SCAUSE_INTERRUPT_BIT;

    /// S-mode software interrupt
    pub const SOFTWARE: usize = SCAUSE_INTERRUPT_BIT | 1;
    /// S-mode timer interrupt
    pub const TIMER: usize = SCAUSE_INTERRUPT_BIT | 5;
    /// S-mode external interrupt (from PLIC)
    pub const EXTERNAL: usize = SCAUSE_INTERRUPT_BIT | 9;

    /// Extract the interrupt cause code (without the interrupt bit)
    pub const fn code(scause: usize) -> usize {
        scause & !SCAUSE_INTERRUPT_BIT
    }

    /// Cause codes without the interrupt bit (for matching after masking)
    pub mod code {
        pub const SOFTWARE: usize = 1;
        pub const TIMER: usize = 5;
        pub const EXTERNAL: usize = 9;
    }
}

/// S-mode exception causes (scause values without interrupt bit)
pub mod exception {
    /// Instruction address misaligned
    pub const INSTRUCTION_MISALIGNED: usize = 0;
    /// Instruction access fault
    pub const INSTRUCTION_ACCESS_FAULT: usize = 1;
    /// Illegal instruction
    pub const ILLEGAL_INSTRUCTION: usize = 2;
    /// Breakpoint
    pub const BREAKPOINT: usize = 3;
    /// Load address misaligned
    pub const LOAD_MISALIGNED: usize = 4;
    /// Load access fault
    pub const LOAD_ACCESS_FAULT: usize = 5;
    /// Store/AMO address misaligned
    pub const STORE_MISALIGNED: usize = 6;
    /// Store/AMO access fault
    pub const STORE_ACCESS_FAULT: usize = 7;
    /// Environment call from U-mode
    pub const ECALL_FROM_U: usize = 8;
    /// Environment call from S-mode
    pub const ECALL_FROM_S: usize = 9;
    // 10-11 reserved
    /// Instruction page fault
    pub const INSTRUCTION_PAGE_FAULT: usize = 12;
    /// Load page fault
    pub const LOAD_PAGE_FAULT: usize = 13;
    // 14 reserved
    /// Store/AMO page fault
    pub const STORE_PAGE_FAULT: usize = 15;
}

/// Check if scause indicates an interrupt
pub const fn is_interrupt(scause: usize) -> bool {
    scause & SCAUSE_INTERRUPT_BIT != 0
}

/// Check if scause indicates an exception
pub const fn is_exception(scause: usize) -> bool {
    scause & SCAUSE_INTERRUPT_BIT == 0
}

// ============================================================================
// Instruction encodings
// ============================================================================

/// WFI (Wait For Interrupt) instruction encoding
pub const INSTR_WFI: u32 = 0x10500073;
