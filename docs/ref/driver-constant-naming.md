# Driver Constant Naming

## Principle

Driver constants are private to their module — the module is the namespace.
Short, unqualified names are unambiguous and read cleanly at call sites.
However, constants that identify the device or describe physical topology
are worth qualifying because they carry meaning beyond the immediate context.

## Rules

**Qualify with a device prefix** (`VIRTIO_`, `SD_`, `PLIC_`):
- Hardware base addresses and IRQ numbers: `VIRTIO_BASE`, `SD_BASE`, `SD_IRQ`
- Structural/sizing constants: `VIRTIO_QUEUE_SIZE`, `VIRTIO_STRIDE`, `VIRTIO_COUNT`
- Platform variants where multiple exist in one file: `QEMU_PLIC_BASE`, `NANO_PLIC_BASE`

**Short names, no prefix** for local protocol detail:
- Register offsets: `REG_STATUS`, `REG_QUEUE_NOTIFY`, `REG_HOST_CONTROL`
- Bit flags and masks: `F_NEXT`, `F_WRITE`, `STATUS_DRIVER_OK`, `XFER_MODE_DMA_ENABLE`
- Commands and request types: `CMD17_READ_SINGLE`, `BLK_T_IN`
- Other single-file constants: timeout values, magic numbers, protocol version IDs

Where a spec or datasheet assigns names to registers, bits, or commands, prefer
those names (adapted to Rust `SCREAMING_SNAKE_CASE`). This makes the code
directly cross-referenceable with the source document without a prefix that
the spec itself does not use.

## Rationale

A constant like `VIRTIO_BASE = 0x10001000` identifies which device and where
it lives — useful context when reading the file top-to-bottom or searching
across the codebase. A constant like `REG_STATUS = 0x070` is register-map
detail; its name only needs to be clear at the call site, where the module
context already tells you which device it belongs to.
