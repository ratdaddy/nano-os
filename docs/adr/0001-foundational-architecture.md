# 0001: Foundational Architecture

- **Status**: Adopted
- **Date**: 2026-05-29
- **Author**: Brian VanLoo

## Context

This ADR establishes the foundational architectural decisions for nano-os as a baseline.
It does not record the history of how these decisions were reached — most were made during
initial development before ADRs were established. Its purpose is to make the current
architecture explicit so that future decisions can be made with clear reference to what
is already decided and why.

Subsequent ADRs will capture decisions made from this point forward.

## Decision

The following decisions are adopted as the architectural baseline.

### Language and Target

**Rust (no_std)** is the implementation language. The crate targets
`riscv64gc-unknown-none-elf` — no standard library, no heap allocator provided by the
runtime, no OS primitives. The kernel provides its own allocator.

This choice is intentional: Rust's ownership model and the `unsafe` discipline it enforces
are valuable for systems code where memory safety cannot be delegated to a runtime.

### Hardware Targets

Two targets are maintained in parallel:

- **QEMU virt** — primary development target; used for all unit tests and integration
  verification via Gates 1–3
- **LicheeRV Nano / SG2002** — physical hardware target; periodic hardware verification
  is required because QEMU does not emulate all hardware behavior

Code must work on both. Hardware-specific behavior (T-Head C906 cache management, MAEE
memory attribute bits, PLIC differences) is documented in `docs/ref/` and must be
handled explicitly rather than assumed away.

### Linux as Primary Design Reference

Linux is the primary design reference for all subsystems. When making a design decision,
check how Linux approaches the same problem first. Deviations from Linux conventions are
permitted but must be documented — either in `docs/design/` (if the deviation affects
system structure) or in an ADR (if it represents a specific decision with alternatives
considered).

This does not mean copying Linux; it means using Linux as a validated starting point
and being explicit when departing from it.

### Execution Model

Two types of schedulable execution context exist:

**Kernel threads** run in S-mode with kernel privileges, share the kernel address space
and page table, and use a minimal saved context (sp, ra, s0–s11 only). They are used
for driver work and other long-running kernel tasks.

**User processes** run in U-mode with isolated address spaces and separate page tables.
They communicate with the kernel via syscalls. Their full register state is saved on
trap entry.

Driver I/O is coordinated through message passing between user processes and kernel
threads. Interrupt handlers are not threads — they run in interrupt context, must be
non-blocking, and communicate with kernel threads via non-blocking message sends.

### Block Subsystem

Three layers separate hardware access from filesystem use:

```
BlockDriver   — hardware interface; async start_read() / start_write()
BlockDisk     — physical disk + request dispatcher
BlockVolume   — logical address spaces (partitions, cached volumes)
```

Filesystems interact only with `BlockVolume`. They have no knowledge of the hardware
layer or dispatch mechanism below it.

### Device Identity

Devices are identified by `(major, minor)` number pairs rather than live pointers.
A `DeviceRegistry` maps `dev_t → Arc<dyn BlockVolume>` (or equivalent for character
devices). This allows device nodes to be created before devfs exists (e.g., in initramfs)
and provides stable identity across driver restarts.

### VFS Abstraction

Four traits define the complete VFS:

- `FileSystem` — registered at boot, one per driver
- `SuperBlock` — one per mounted filesystem instance
- `InodeOps` — per-inode operations (lookup, create, etc.)
- `FileOps` — per-open-file operations (read, write, seek)

`Arc<Inode>` is the ownership model throughout the VFS. An `Inode` wraps an `Arc<dyn
InodeOps>` and carries metadata (type, permissions). An LRU cache keyed by inode number
manages inode lifetime.

## Consequences

- All new subsystem design begins from Linux conventions; departures are documented
- The block/VFS layering creates a stable API surface between subsystems; changes to
  the lower layers do not propagate to filesystem code
- Hardware-specific behavior must be handled explicitly — there is no portability layer
  that abstracts it away

## Alternatives Considered

- **C** — rejected in favor of Rust for ownership and unsafe discipline
- **Direct hardware pointers for device identity** — rejected; major/minor numbers allow
  device nodes to exist before devices are initialized
- **Single monolithic VFS type** — rejected; the trait hierarchy allows filesystems to
  be developed and tested independently

## References

- [ADR 0000](0000-adr-process-and-template-usage.md) — ADR process
- `docs/design/vfs-design.md` — VFS trait hierarchy and ownership model
- `docs/design/block-device-design.md` — block subsystem layer architecture
- `docs/design/kernel-threads-architecture.md` — execution model and scheduling
- `docs/ref/thead-c906-memory-guide.md` — T-Head C906 hardware constraints
