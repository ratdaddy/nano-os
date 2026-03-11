# nano-os

A minimal RISC-V operating system for the LicheeRV Nano (SG2002) and QEMU.

## Features

- **Process management**: ELF loading, user-mode processes, syscalls (write, exit, yield, read)
- **Virtual file system**: ramfs, procfs, character devices
- **Kernel threads**: UART writer, idle thread, user process threads
- **Interrupts**: PLIC, UART RX/TX
- **Memory management**: Page tables, kernel/user address space separation
- **SD card I/O**: SDHCI driver for reading FAT32 filesystem on LicheeRV Nano

## Build and Run

### Prerequisites

- Rust nightly with `riscv64imac-unknown-none-elf` target
- QEMU (`qemu-system-riscv64`)
- Docker (for initramfs and SD image builds)
- RISC-V GNU toolchain (`riscv64-unknown-elf-ld`, `rust-objcopy`)
- U-Boot mkimage (for LicheeRV Nano boot image)

### Quick Start

**QEMU:**
```bash
make run
```

**LicheeRV Nano:**
```bash
# Build boot image and copy to SD card (macOS)
make copy
```

> **First time?** See [Preparing an SD card](#preparing-an-sd-card) below.

## Makefile Targets

### Build Targets

| Target | Description |
|--------|-------------|
| `make` or `make all` | Build kernel and boot image for LicheeRV Nano |
| `make run` | Build and run in QEMU |
| `make test` | Run kernel tests |
| `make initramfs` | Build initramfs.cpio (requires Docker) |
| `make sdimg` | Create sd.img with FAT32 partition (requires Docker) |

### Hardware Targets

| Target | Description |
|--------|-------------|
| `make copy` | Build and copy boot.sd + fip.bin to mounted SD card, then eject |
| `make monitor-cmds` | Print U-Boot monitor commands for manual loading |

### Debugging

| Target | Description |
|--------|-------------|
| `make qemu-debug` | Start QEMU with GDB server on port 1234 |
| `make gdb` | Launch riscv64-gdb in Docker container |

### Docker Images

| Target | Description |
|--------|-------------|
| `make initramfs-docker` | Build Docker image for initramfs creation |
| `make sdimg-docker` | Build Docker image for SD card image creation |
| `make gdb-docker` | Build Docker image for RISC-V GDB |

### Utilities

| Target | Description |
|--------|-------------|
| `make clean` | Remove build artifacts |
| `make lint` | Run clippy |
| `make format` | Run rustfmt |

## Debugging with GDB

1. Start QEMU with GDB server:
   ```bash
   make qemu-debug
   ```

2. In another terminal, run GDB:
   ```bash
   make gdb
   ```

3. Set breakpoints and debug:
   ```gdb
   b *0x80200000
   x/20i $pc
   x/20gx 0x80201000  # Page table dump
   c
   ```

## Boot Menu

When the kernel boots, it presents an interactive menu:

### Process Options
- **1)** Run one process
- **2)** Run two processes (demonstrates multi-process yield)

### Demos
- **3)** Thread message passing
- **4)** UART RX interrupts
- **5)** UART TX flood

### Inspect
- **6)** Mount table
- **7)** Filesystem contents (VFS tree)
- **8)** ELF headers of `/prog_example`
- **9)** Procfs contents (`/proc`)

### Hardware (LicheeRV Nano only)
- **s)** SD card controller registers and FAT32 filesystem inspection

## Architecture

- **Target**: `riscv64imac-unknown-none-elf` (RV64 with atomics and compressed instructions)
- **Memory**: Kernel at `0x8020_0000`, user processes at `0x1000_0000`
- **Initramfs**: CPIO archive loaded at boot, mounted as root filesystem
- **Page size**: 4 KiB

## Hardware Support

### LicheeRV Nano (SG2002)
- UART0 at `0x0411_0000`
- PLIC at `0x7000_0000`
- SD controller (SDHCI 4.2) at `0x0431_0000`
- Custom device tree (`bootdata/sg2002.dtb`)

### QEMU virt machine
- UART (16550) at `0x1000_0000`
- PLIC at `0x0c00_0000`
- Memory: 256 MiB

## File Structure

```
nano-os/
├── src/
│   ├── kernel_main.rs      # Boot menu and entry point
│   ├── syscall/            # System call handlers
│   ├── vfs.rs              # Virtual file system
│   ├── ramfs.rs            # RAM-based filesystem
│   ├── procfs.rs           # /proc filesystem
│   ├── drivers/            # UART, PLIC
│   ├── kthread/            # Kernel threads
│   └── demos/              # Interactive demos (SD read, VFS inspect, etc.)
├── prog_example/           # User-space test program
├── bootdata/               # Device trees, U-Boot config
├── initramfs/              # Docker build for initramfs
└── sdimg/                  # Docker build for SD card image

```

## Preparing an SD card

`make copy` assumes the SD card already has the correct partition layout. Run these
steps once on a new card before using `make copy`.

1. Insert the card and find its device node:
   ```bash
   diskutil list
   ```
   Look for your card, e.g. `/dev/disk4`.

2. Partition it with the layout the Makefile expects:
   ```bash
   diskutil partitionDisk /dev/disk4 MBR \
     FAT32 KERNEL 64M \
     FAT32 BOOT R
   ```
   This creates partition 1 as FAT32 labelled `KERNEL` (macOS mounts it at
   `/Volumes/KERNEL`) and partition 2 as a placeholder FAT32. The type of
   partition 2 doesn't matter — `make copy` will overwrite it with the ext3
   image via `dd`.

3. Build the Docker images if you haven't already:
   ```bash
   make initramfs-docker
   make ext2img-docker
   ```

The card is now ready. `make copy` will:
- Copy `boot.sd` and `fip.bin` to the FAT32 partition
- `dd` the ext3 filesystem image (`ext2.img`) directly onto partition 2

## SD Card Image Format

The `make sdimg` target creates a 64 MiB disk image with:
- **MBR partition table**
- **Partition 1**: FAT32, starts at sector 2048, label "KERNEL"

This matches the layout of the real LicheeRV Nano SD card.

## Trace Features

Enable detailed tracing by uncommenting in the Makefile:

```makefile
FEATURES := --features print_dtb,trace_syscalls,trace_trap,trace_scheduler,trace_process
```

Available features:
- `print_dtb`: Print device tree at boot
- `trace_syscalls`: Log all system calls
- `trace_trap`: Log trap entry/exit
- `trace_scheduler`: Log context switches
- `trace_process`: Log process lifecycle
- `trace_amo`: Log atomic operations

## License

MIT
