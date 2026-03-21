# Kernel Memory Map

This document describes the virtual memory layout used by the kernel.
Addresses shown are the lower 32 bits; prefix with `0xffff_ffff_` for high-half addresses.

## L2 Page Table Entries (VPN2 indices)

### VPN2 0x000: 0x0000_0000 - 0x3fff_ffff (Low memory, identity mapped)

MMIO regions (platform-specific):

| Region | NanoRV (LicheeRV) | QEMU virt |
|--------|-------------------|-----------|
| UART   | 0x0414_0000 - 0x0414_1000 | 0x1000_0000 - 0x1000_1000 |
| PLIC   | 0x7000_0000 - 0x7040_0000 | 0x0c00_0000 - 0x0c40_0000 |

### VPN2 0x002: 0x8000_0000 - 0xbfff_ffff (Physical RAM)

- Identity mapped from DTB-specified memory region
- Typically 0x8000_0000 - 0x9000_0000 (256 MB) or as specified by hardware

### VPN2 0x100: 0xffff_ffc0_0000_0000 - 0xffff_ffc0_3fff_ffff (Process load area)

- Kernel maps ELF segments here during process loading
- Pages are then transferred to user address space
- Virtual address: `PROCESS_LOAD_AREA = 0xffff_ffc0_0000_0000`

### VPN2 0x1fe: 0xffff_ffff_8000_0000 - 0xffff_ffff_bfff_ffff (Kernel code)

- `.text` - Kernel executable code (RX)
- `.rodata` - Read-only data (R)
- `.data` - Initialized data (RW)
- `.bss` - Zero-initialized data (RW)

### VPN2 0x1ff: 0xffff_ffff_c000_0000 - 0xffff_ffff_ffff_ffff (Kernel runtime)

| Address Range | Size | Description |
|---------------|------|-------------|
| 0xc000_0000 - ... | grows | Kernel heap (grows upward) |
| 0xffc0_0000 - 0xffe0_0000 | 2 MB max | Kernel stack region (grows downward) |
| 0xffe0_0000 - 0xffe0_1000 | 4 KB | Trampoline trap frame |
| 0xfffe_0000 - 0xfffe_1000 | 4 KB | Process trampoline code |

Notes:
- `KERNEL_HEAP_START = 0xffff_ffff_c000_0000`
- `KERNEL_STACK_START = 0xffff_ffff_ffe0_0000` (top of stack, grows down)
- `KERNEL_STACK_END = 0xffff_ffff_ffc0_0000` (maximum stack extent)
- `TRAMPOLINE_TRAP_FRAME = 0xffff_ffff_ffe0_0000` (adjacent to stack top)

## User Process Memory (separate page table)

| Address Range | Description |
|---------------|-------------|
| 0x0000_0000 - ... | User code and data (from ELF) |
| ... - 0xffe0_0000 | User stack (grows downward from `PROCESS_STACK_START`) |
| 0xfffe_0000 - 0xfffe_1000 | Process trampoline (shared with kernel) |

## See Also

- `src/kernel_memory_map.rs` - Kernel memory mapping implementation
- `src/process_memory_map.rs` - User process memory setup
- `link.ld` - Linker script defining section layout
- `notes/thead-c906-memory-guide.md` - T-Head C906 memory attributes
