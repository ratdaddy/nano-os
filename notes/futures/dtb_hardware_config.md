# Table-Driven DTB Parsing

## Current Problem

### Multiple Traversals
Each time we need new DTB data, we add another traversal:
- `collect_memory_map()` - one full traversal
- `detect_cpu_type()` - another full traversal
- `parse_timebase_frequency()` - yet another full traversal

As we add more hardware configuration (UART base, PLIC base, VirtIO devices, etc.), this becomes increasingly inefficient. We're walking the same tree structure repeatedly.

### Hardcoded Hardware Configuration
Several drivers hardcode base addresses and IRQ numbers with per-platform constants selected by `dtb::get_cpu_type()`:

**UART** (`src/drivers/uart.rs`)
- QEMU: base `0x1000_0000`, reg_shift 0, reg_io_width 1
- NanoKVM: base `0x0414_0000`, reg_shift 2, reg_io_width 4
- Should parse from DTB: `compatible = "ns16550a"` or `"snps,dw-apb-uart"`, fields `reg`, `reg-shift`, `reg-io-width`

**PLIC** (`src/drivers/plic.rs`)
- QEMU: base `0x0c00_0000`, UART IRQ 10
- NanoKVM: base `0x7000_0000`, UART IRQ 0x2c
- Should parse from DTB: `compatible = "riscv,plic0"`, field `reg`; UART IRQ from uart node's `interrupts` property

**Memory map** (`src/kernel_memory_map.rs`)
- Hardcodes same UART and PLIC addresses for page table mappings
- Should derive from DTB-parsed values

### Code Bloat
Adding new properties requires modifying the parser code with new functions and match statements, leading to growing complexity.

## Proposed Solution: Table-Driven Single-Pass Parser

Parse the DTB **once** at boot using a static table of property handlers. Each subsystem declares what it needs via table entries; the parser iterates through handlers on each property.

### Core Data Structures

```rust
// Single struct containing all DTB-derived data
pub struct DtbInfo {
    pub cpu_type: CpuType,
    pub timebase_freq: u64,
    pub memory: Option<memory::Region>,
    pub reserved: heapless::Vec<memory::Region, 16>,
    pub initrd_start: usize,
    pub initrd_end: usize,
    pub uart_base: Option<usize>,
    pub uart_reg_shift: u32,
    pub uart_reg_io_width: u32,
    pub uart_irq: Option<u32>,
    pub plic_base: Option<usize>,
    pub virtio_devices: heapless::Vec<VirtioMmioDevice, 8>,

    // Internal state for multi-property parsing
    pending_initrd_start: Option<usize>,
    pending_initrd_end: Option<usize>,
}

// Handler for a single property
struct PropertyHandler {
    /// Node name pattern ("" = any, "serial@" = prefix match, "cpus" = exact)
    node_pattern: &'static str,
    /// Property name to match
    prop_name: &'static str,
    /// Expected data length (0 = any length)
    expected_len: usize,
    /// Callback to populate DtbInfo
    handler: fn(data: *const u8, len: usize, info: &mut DtbInfo),
}
```

### Static Handler Table

All DTB parsing logic is declared in one static table:

```rust
static PROPERTY_HANDLERS: &[PropertyHandler] = &[
    PropertyHandler {
        node_pattern: "cpus",
        prop_name: "timebase-frequency",
        expected_len: 4,
        handler: |data, _len, info| {
            info.timebase_freq = unsafe { read_be32(data) as u64 };
        },
    },
    PropertyHandler {
        node_pattern: "memory@",
        prop_name: "reg",
        expected_len: 16,
        handler: |data, _len, info| {
            let start = unsafe { read_be64(data) as usize };
            let size = unsafe { read_be64(data.add(8)) as usize };
            info.memory = Some(memory::Region { start, end: start + size });
        },
    },
    PropertyHandler {
        node_pattern: "serial@",
        prop_name: "reg",
        expected_len: 16,
        handler: |data, _len, info| {
            info.uart_base = Some(unsafe { read_be64(data) as usize });
        },
    },
    PropertyHandler {
        node_pattern: "serial@",
        prop_name: "reg-shift",
        expected_len: 4,
        handler: |data, _len, info| {
            info.uart_reg_shift = unsafe { read_be32(data) };
        },
    },
    PropertyHandler {
        node_pattern: "serial@",
        prop_name: "reg-io-width",
        expected_len: 4,
        handler: |data, _len, info| {
            info.uart_reg_io_width = unsafe { read_be32(data) };
        },
    },
    PropertyHandler {
        node_pattern: "serial@",
        prop_name: "interrupts",
        expected_len: 4,
        handler: |data, _len, info| {
            info.uart_irq = Some(unsafe { read_be32(data) });
        },
    },
    PropertyHandler {
        node_pattern: "plic@",
        prop_name: "reg",
        expected_len: 16,
        handler: |data, _len, info| {
            info.plic_base = Some(unsafe { read_be64(data) as usize });
        },
    },
    PropertyHandler {
        node_pattern: "chosen",
        prop_name: "linux,initrd-start",
        expected_len: 8,
        handler: |data, _len, info| {
            info.pending_initrd_start = Some(unsafe { read_be64(data) as usize });
            commit_initrd_if_ready(info);
        },
    },
    PropertyHandler {
        node_pattern: "chosen",
        prop_name: "linux,initrd-end",
        expected_len: 8,
        handler: |data, _len, info| {
            info.pending_initrd_end = Some(unsafe { read_be64(data) as usize });
            commit_initrd_if_ready(info);
        },
    },
];

fn commit_initrd_if_ready(info: &mut DtbInfo) {
    if let (Some(start), Some(end)) = (info.pending_initrd_start, info.pending_initrd_end) {
        info.initrd_start = start;
        info.initrd_end = end;
    }
}
```

### Single-Pass Parser

```rust
pub fn parse_dtb_info(dtb: *const u8) -> DtbInfo {
    let ctx = unsafe { parse_dtb(dtb) };
    let mut info = DtbInfo::default();
    let mut current_node = "";

    unsafe {
        traverse_dtb(&ctx, |token, _depth, name_opt, prop_opt| {
            match token {
                DtbToken::BeginNode => {
                    if let Some(name) = name_opt {
                        current_node = name;
                    }
                }
                DtbToken::Prop => {
                    if let Some((prop_name, data, len)) = prop_opt {
                        // Check all handlers - O(n×m) where n = properties, m = handlers
                        for handler in PROPERTY_HANDLERS {
                            if matches_node(current_node, handler.node_pattern)
                                && prop_name == handler.prop_name
                                && (handler.expected_len == 0 || len == handler.expected_len)
                            {
                                (handler.handler)(data, len, &mut info);
                            }
                        }
                    }
                }
                _ => {}
            }
        });
    }

    info
}

fn matches_node(node: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return true; // Match any node
    }
    if pattern.ends_with('@') {
        node.starts_with(&pattern[..pattern.len() - 1])
    } else {
        node == pattern
    }
}
```

### Usage

```rust
// In kernel boot (main.rs)
static DTB_INFO: Mutex<Option<DtbInfo>> = Mutex::new(None);

pub fn init_dtb(dtb: *const u8) {
    let info = dtb::parse_dtb_info(dtb);
    *DTB_INFO.lock() = Some(info);
}

// Getter functions
pub fn get_cpu_type() -> CpuType {
    DTB_INFO.lock().as_ref().unwrap().cpu_type
}

pub fn get_timebase_frequency() -> u64 {
    DTB_INFO.lock().as_ref().unwrap().timebase_freq
}

pub fn get_uart_base() -> Option<usize> {
    DTB_INFO.lock().as_ref().and_then(|info| info.uart_base)
}

pub fn get_uart_config() -> Option<(usize, u32, u32, u32)> {
    DTB_INFO.lock().as_ref().and_then(|info| {
        info.uart_base.and_then(|base| {
            info.uart_irq.map(|irq| {
                (base, info.uart_reg_shift, info.uart_reg_io_width, irq)
            })
        })
    })
}
```

## How to Extend

### Adding Simple Property

To add CLINT base address:

1. Add field to `DtbInfo`:
```rust
pub clint_base: Option<usize>,
```

2. Add handler to table:
```rust
PropertyHandler {
    node_pattern: "clint@",
    prop_name: "reg",
    expected_len: 16,
    handler: |data, _len, info| {
        info.clint_base = Some(unsafe { read_be64(data) as usize });
    },
},
```

3. Add getter:
```rust
pub fn get_clint_base() -> Option<usize> {
    DTB_INFO.lock().as_ref().and_then(|info| info.clint_base)
}
```

That's it - no changes to parsing logic, no new traversals.

### Adding Multi-Property Node

For properties that require multiple values (like VirtIO devices with base + IRQ):

1. Define struct and add to `DtbInfo`:
```rust
#[derive(Debug, Clone, Copy)]
pub struct VirtioMmioDevice {
    pub base: usize,
    pub irq: u32,
}

pub struct DtbInfo {
    // ...
    pub virtio_devices: heapless::Vec<VirtioMmioDevice, 8>,

    // Internal state
    pending_virtio_base: Option<usize>,
    pending_virtio_irq: Option<u32>,
}
```

2. Add handlers for both properties:
```rust
PropertyHandler {
    node_pattern: "virtio_mmio@",
    prop_name: "reg",
    expected_len: 16,
    handler: |data, _len, info| {
        info.pending_virtio_base = Some(unsafe { read_be64(data) as usize });
        commit_virtio_if_ready(info);
    },
},
PropertyHandler {
    node_pattern: "virtio_mmio@",
    prop_name: "interrupts",
    expected_len: 4,
    handler: |data, _len, info| {
        info.pending_virtio_irq = Some(unsafe { read_be32(data) });
        commit_virtio_if_ready(info);
    },
},

fn commit_virtio_if_ready(info: &mut DtbInfo) {
    if let (Some(base), Some(irq)) = (info.pending_virtio_base, info.pending_virtio_irq) {
        let _ = info.virtio_devices.push(VirtioMmioDevice { base, irq });
        info.pending_virtio_base = None;
        info.pending_virtio_irq = None;
    }
}
```

### Handling Multiple Instances

For collecting multiple nodes of the same type (e.g., all VirtIO devices), the handler needs to detect node boundaries. This requires enhancement to track `EndNode`:

```rust
pub fn parse_dtb_info(dtb: *const u8) -> DtbInfo {
    // ... as before, but add:

    DtbToken::EndNode => {
        // If leaving a virtio node, commit any pending device
        if current_node.starts_with("virtio_mmio@") {
            commit_virtio_if_ready(&mut info);
        }
        current_node = "";
    }
}
```

## Optional: Macro for Cleaner Syntax

```rust
macro_rules! dtb_handler {
    ($node:expr, $prop:expr, $len:expr => |$data:ident, $info:ident| $body:block) => {
        PropertyHandler {
            node_pattern: $node,
            prop_name: $prop,
            expected_len: $len,
            handler: |$data, _len, $info| $body,
        }
    };
}

static PROPERTY_HANDLERS: &[PropertyHandler] = &[
    dtb_handler!("cpus", "timebase-frequency", 4 => |data, info| {
        info.timebase_freq = unsafe { read_be32(data) as u64 };
    }),

    dtb_handler!("serial@", "reg", 16 => |data, info| {
        info.uart_base = Some(unsafe { read_be64(data) as usize });
    }),
];
```

## Trade-offs

### Advantages
- **Single traversal**: Parse DTB once, extract everything
- **Easy to extend**: Add table entry, no parser changes
- **Declarative**: All parsing logic visible in one place
- **No code bloat**: Parser stays simple regardless of property count
- **No-std friendly**: Static table, bounded collections
- **Eliminates hardcoded addresses**: All hardware config from DTB

### Disadvantages
- **Handler overhead**: O(n×m) where n = properties, m = handlers (but n and m are both small)
- **All-or-nothing**: Must define struct upfront (though can always add fields)
- **Memory for unused data**: Struct contains all fields even if some platform doesn't use them (minimal cost)

## Implementation Notes

- Keep `pending_*` fields private to prevent external access to internal state
- Use `#[inline]` on getters for zero-cost abstraction
- Consider splitting very large handler tables by subsystem if readability suffers
- For rare/complex parsing (like reserved memory regions), can still use dedicated code path
- Pattern matching is simple string comparison - fast even with many handlers
- Struct fields use `Option<T>` for optional hardware (e.g., not all boards have VirtIO)

## Migration Path

1. Implement `DtbInfo` struct with current fields (cpu_type, timebase_freq, memory, reserved, initrd)
2. Implement table-driven parser alongside existing functions
3. Switch call sites to use new getters
4. Remove old traversal functions
5. Add hardware config fields (uart_base, plic_base, etc.)
6. Update drivers to use DTB-parsed values instead of hardcoded constants
7. Add new properties via table entries going forward
