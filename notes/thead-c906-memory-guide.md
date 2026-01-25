# T-Head C906 Memory Configuration Guide

This document describes the memory configuration requirements for the T-Head XuanTie C906 RISC-V processor, based on the XTheadMae (Memory Attribute Extension) and XTheadCmo (Cache Management Operations) specifications.

## Overview

The T-Head C906 uses custom RISC-V extensions for memory attributes that differ from the standard RISC-V approach. When the MAEE (Memory Attribute Extension Enable) bit is set in the `mxstatus` CSR, the processor interprets bits 59-63 of page table entries (PTEs) as memory attribute flags.

**Important**: These bits conflict with the later standardized Svpbmt and Svnapot extensions. You cannot use XTheadMae simultaneously with these standard extensions.

## Prerequisites

The MAEE bit (bit 21) in the `mxstatus` CSR must be enabled by M-mode firmware (e.g., OpenSBI) before S-mode can use the memory attribute extension. Without MAEE enabled, setting these PTE bits will cause crashes.

Detection: Check if `th.sxstatus.MAEE` (bit 21) equals 1.

## Page Table Entry Memory Attributes

The T-Head C906 uses PTE bits [59:63] for memory attributes:

| Bit | Name | Description |
|-----|------|-------------|
| 63 | SO | Strong Order - ensures sequential read/write execution |
| 62 | C | Cacheable - allows caching of memory contents |
| 61 | B | Bufferable - allows write buffering |
| 60 | SH | Shareable - enables multi-processor sharing |
| 59 | Sec | Trustable/Secure |

### Memory Type Encodings

| Memory Type | SO | C | B | Encoding | Use Case |
|-------------|----|----|---|----------|----------|
| Normal Cacheable | 0 | 1 | 1 | `0x6 << 61` | RAM, heap, stack |
| Non-cacheable | 0 | 0 | 1 | `0x2 << 61` | Shared memory |
| Bufferable Device | 1 | 0 | 1 | `0xA << 61` | Fast I/O |
| Strongly Ordered | 1 | 0 | 0 | `0x8 << 61` | MMIO, PLIC |

### Recommended Flag Combinations

```c
// Normal memory (RAM, heap, stack, kernel data)
#define THEAD_MEMORY  (0x0F << 59)  // C=1, B=1, SH=1, Sec=1

// I/O and PLIC memory (strongly ordered)
#define THEAD_SO      (1 << 63)     // SO=1 only
```

## Critical Requirements

### 1. Cacheable Memory for Atomics

**The C906 requires memory to be marked Cacheable (C=1) for AMO (Atomic Memory Operations) instructions to work.**

Without the Cacheable flag, atomic operations like `amoadd`, `amoswap`, `amoor` will trigger:
- Exception code 7: Store/AMO access fault

This affects:
- Spinlocks (spin crate)
- AtomicUsize/AtomicU8 operations
- Any synchronization primitives using atomics

### 2. Strong Ordering for MMIO

Device memory (UART, PLIC, etc.) must be mapped with Strong Order (SO=1) to prevent:
- Out-of-sequence memory access
- "Leaky reads" contaminating adjacent registers
- PLIC interrupt claims returning zero
- UART data appearing unavailable

### 3. Explicit Cache Flags Required

Unlike standard RISC-V, the C906 does **not** default to cacheable memory. All kernel memory regions must explicitly set the C and B flags:
- Kernel text (.text)
- Kernel read-only data (.rodata)
- Kernel data (.data)
- Kernel BSS (.bss)
- Kernel stack
- Kernel heap

Without these flags, performance degrades dramatically (benchmarks show 100x slowdown).

## Cache Management Operations (XTheadCmo)

The C906 provides custom cache management instructions. These are necessary after modifying page tables.

### Instruction Encodings

| Instruction | Encoding | Description |
|-------------|----------|-------------|
| `th.dcache.call` | `0x0020000b` | Clean (writeback) all D-cache |
| `th.dcache.iall` | `0x0010000b` | Invalidate all D-cache |
| `th.dcache.ciall` | `0x0030000b` | Clean and invalidate all D-cache |
| `th.icache.iall` | `0x0100000b` | Invalidate all I-cache |
| `th.icache.ialls` | `0x0110000b` | Invalidate all I-cache (broadcast to all cores) |

### Usage in Rust

```rust
/// Flush D-cache only (for page table mods within same address space)
pub fn thead_flush_dcache() {
    unsafe {
        core::arch::asm!(
            ".long 0x0030000b",   // th.dcache.ciall - clean AND invalidate D-cache
            options(nostack, preserves_flags),
        );
    }
}

/// Flush both caches (for SATP switches between address spaces)
pub fn thead_flush_cache_for_satp_switch() {
    unsafe {
        core::arch::asm!(
            ".long 0x0030000b",   // th.dcache.ciall - clean and invalidate D-cache
            ".long 0x0100000b",   // th.icache.iall  - invalidate I-cache
            options(nostack, preserves_flags),
        );
    }
}
```

### When to Use Cache Operations

1. **After modifying page tables (same address space)**: `dcache.ciall` + `sfence.vma`
   - Example: growing heap or stack
2. **Before SATP switch (different address space)**: `dcache.ciall` + `icache.iall` + `sfence.vma`
   - Example: entering user process, returning from trap
3. **After loading code**: `dcache.ciall` + `icache.iall` before executing
4. **For DMA coherence**: Call appropriate cache operations before/after DMA

### Recommended Sequence After Page Table Modification

```rust
fn grow_heap() {
    // 1. Modify page table entries
    // 2. Clean and invalidate D-cache
    // 3. Execute sfence.vma to flush TLB

    thead_flush_dcache();

    unsafe {
        core::arch::asm!("sfence.vma zero, zero", options(nostack));
    }
}
```

## Known Issues and Workarounds

### Issue: dcache.call (clean only) Corrupts Return Address

**Symptom**: After calling `th.dcache.call` (0x0020000b), function returns to address 0x0.

**Root Cause**: Using `dcache.call` (clean/writeback only) instead of `dcache.ciall` (clean AND invalidate) can corrupt the return address during heap growth operations.

**Solution**: Always use `dcache.ciall` (0x0030000b) instead of `dcache.call` (0x0020000b) for page table modifications. The clean-and-invalidate operation is required, not just clean.

### Issue: AMO Fault on Memory Without C Flag

**Symptom**: `scause = 0x7` (Store/AMO access fault) when using Mutex or atomics.

**Solution**: Ensure all memory regions where atomics may be used have the Cacheable (C) flag set in the PTE.

## Example: Complete Memory Mapping Setup

```rust
// Page flags for T-Head C906
pub struct PageFlags {
    bits: usize,
}

impl PageFlags {
    // Standard RISC-V flags
    pub const VALID: Self = Self { bits: 1 << 0 };
    pub const READ: Self = Self { bits: 1 << 1 };
    pub const WRITE: Self = Self { bits: 1 << 2 };
    pub const EXECUTE: Self = Self { bits: 1 << 3 };
    pub const USER: Self = Self { bits: 1 << 4 };
    pub const GLOBAL: Self = Self { bits: 1 << 5 };
    pub const ACCESSED: Self = Self { bits: 1 << 6 };
    pub const DIRTY: Self = Self { bits: 1 << 7 };

    // T-Head C906 memory attributes (bits 59-63)
    // Normal cacheable memory: C=1, B=1, SH=1, Sec=1
    pub const THEAD_MEMORY: Self = Self { bits: 0x0F << 59 };
    // Strongly ordered (for MMIO): SO=1
    pub const THEAD_SO: Self = Self { bits: 1 << 63 };
}

// Map kernel data with T-Head memory flags
fn map_kernel_data(data_start: usize, data_end: usize, phys_start: usize) {
    let flags = if is_thead_c906() {
        PageFlags::READ | PageFlags::WRITE |
        PageFlags::ACCESSED | PageFlags::DIRTY |
        PageFlags::THEAD_MEMORY
    } else {
        PageFlags::READ | PageFlags::WRITE |
        PageFlags::ACCESSED | PageFlags::DIRTY
    };

    map_range(data_start, phys_start, data_end, flags, PageSize::Size4K);
}

// Map MMIO with strong ordering
fn map_mmio(mmio_start: usize, mmio_end: usize) {
    let flags = if is_thead_c906() {
        PageFlags::READ | PageFlags::WRITE |
        PageFlags::ACCESSED | PageFlags::DIRTY |
        PageFlags::THEAD_SO
    } else {
        PageFlags::READ | PageFlags::WRITE |
        PageFlags::ACCESSED | PageFlags::DIRTY
    };

    map_range(mmio_start, mmio_start, mmio_end, flags, PageSize::Size4K);
}
```

## References

- [T-Head Extension Specification](https://github.com/T-head-Semi/thead-extension-spec) - Official XTheadMae and XTheadCmo documentation
- [XuanTie OpenC906 User Manual](https://occ-intl-prod.oss-ap-southeast-1.aliyuncs.com/resource/XuanTie-OpenC906-UserManual.pdf) - Official hardware documentation
- [OpenC906 GitHub Repository](https://github.com/XUANTIE-RV/openc906) - RTL source and documentation
- [NuttX T-Head C906 MMU Configuration](https://github.com/apache/nuttx/pull/12722) - Reference implementation
- [QEMU XTheadMaee Support](https://www.mail-archive.com/qemu-devel@nongnu.org/msg1019755.html) - QEMU emulation details
- [Ox64 BL808 PLIC Configuration](https://lupyuen.org/articles/plic3.html) - Practical example with troubleshooting
