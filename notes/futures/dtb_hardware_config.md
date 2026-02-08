# Parse Hardware Configuration from DTB

Several drivers hardcode base addresses and IRQ numbers with per-platform constants selected by `dtb::get_cpu_type()`. These should be parsed from the device tree blob instead.

## UART (`src/drivers/uart.rs`)

Hardcoded configs:
- QEMU: base `0x1000_0000`, reg_shift 0, reg_io_width 1
- NanoKVM: base `0x0414_0000`, reg_shift 2, reg_io_width 4

DTB nodes to parse: `compatible = "ns16550a"` or `compatible = "snps,dw-apb-uart"`. Fields: `reg` (base address), `reg-shift`, `reg-io-width`.

## PLIC (`src/drivers/plic.rs`)

Hardcoded configs:
- QEMU: base `0x0c00_0000`, UART IRQ 10
- NanoKVM: base `0x7000_0000`, UART IRQ 0x2c

DTB nodes to parse: `compatible = "riscv,plic0"`. Fields: `reg` (base address). UART IRQ number comes from the uart node's `interrupts` property.

## Memory map (`src/kernel_memory_map.rs`)

Hardcodes the same UART and PLIC addresses for page table mappings:
- QEMU UART: `0x1000_0000`
- NanoKVM UART: `0x0414_0000`
- QEMU PLIC: `0x0c00_0000` - `0x0c40_0000`
- NanoKVM PLIC: `0x7000_0000` - `0x7040_0000`

These should derive from the same DTB-parsed values used by the drivers.

## Current DTB parsing (`src/dtb.rs`)

Currently only extracts CPU type (`get_cpu_type()`). Would need to be extended to parse `reg`, `interrupts`, `reg-shift`, and `reg-io-width` properties from device nodes and expose them through a structured API.
