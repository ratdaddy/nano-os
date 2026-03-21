# Kernel Boot Stages and Interrupt Handling

High-level overview of boot sequence, tracking CPU mode, interrupt state, trap handlers, memory mapping, and console implementation.

## Quick Reference: Interrupt State Changes

| Stage | CPU Mode | sstatus.SIE | sie Register | Interrupts? | Active Trap Handler |
|-------|----------|-------------|--------------|-------------|---------------------|
| M-mode firmware | M-mode | N/A | N/A | Varies | mtvec (M-mode handler) |
| _start trampoline | S-mode | 0 | 0 | **DISABLED** | None (no stvec set yet) |
| rust_main | S-mode | 0 | 0 | **DISABLED** | boot_trap_handler |
| kernel_main | S-mode | 0 | 0 | **DISABLED** | kernel_trap_entry |
| enter_process | S-mode | 0 | 0x202 | **DISABLED** | Changes to trap_entry before sret |
| User mode | U-mode | 1 | 0x202 | **ENABLED** | trap_entry |
| During syscall/trap | S-mode | 0 (hw clears) | 0x202 | **DISABLED** | trap_entry |
| After sret to user | U-mode | 1 (restored) | 0x202 | **ENABLED** | trap_entry |

**Key Hardware Behavior**: RISC-V hardware **automatically clears sstatus.SIE** when a trap occurs, and restores it from sstatus.SPIE on sret. This provides implicit mutual exclusion for all kernel code!

---

## Stage 0: M-Mode Firmware (OpenSBI/U-Boot)

**CPU Mode**: M-mode
**Code**: OpenSBI or U-Boot SPL

### State
- **Interrupts**: Varies (firmware-specific)
- **Memory Map**: Identity mapped
- **Trap Handler**: mtvec (M-mode handler)
- **Console**: M-mode SBI handlers

### What Happens
- Initializes hardware (memory controller, timers, etc.)
- Sets up M-mode trap delegation (medeleg, mideleg)
- T-Head C906: Sets MXSTATUS.MAEE, MHCR, MCOR registers
- Loads kernel ELF to physical address 0x80200000
- Prepares to enter S-mode (mepc = _start, a0 = hart_id, a1 = dtb_ptr)
- Executes mret → S-mode, jumps to _start

---

## Stage 1: _start - Kernel Entry Trampoline

**File**: `src/trampoline.rs`
**CPU Mode**: S-mode
**Entry**: Physical address 0x80200000

### State
- **Interrupts**: **DISABLED** (sstatus.SIE = 0, sie = 0)
- **Memory Map**: **Bare mode** (satp = 0, physical = virtual)
- **Trap Handler**: **None** (stvec not set - DANGER!)
- **Stack**: None initially, sets up 64 KB boot stack
- **Console**: SBI available (not used)
- **Heap**: Not available

### What Happens
- Saves firmware arguments (hart_id, dtb_ptr)
- Sets up 64 KB boot stack in low physical memory
- Creates minimal root page table with two 1 GB gigapages:
  - Identity map: VA 0x80000000-0xC0000000 → PA 0x80000000-0xC0000000 (DRAM region)
  - High-half map: VA 0xFFFFFFC000000000-0xFFFFFFC040000000 → PA 0x80000000-0xC0000000
- Enables paging (csrw satp, sfence.vma)
- Adds kernel physical bounds to arguments (a2, a3)
- Jumps to rust_main

**Critical**: Runs with no trap handler - any bug would crash!

---

## Stage 2: rust_main() - Early S-Mode Boot

**File**: `src/main.rs`
**CPU Mode**: S-mode

### State
- **Interrupts**: **DISABLED** (sstatus.SIE = 0, sie = 0)
- **Memory Map**: Boot page table (identity + high-half gigapages)
- **Trap Handler**: boot_trap_handler (minimal panic handler)
- **Stack**: Boot stack (64 KB in low memory)
- **Console**: **SBI console** (println! → sbi_console_putchar)
- **Heap**: Not available initially, set up during stage

### What Happens
- Sets stvec to boot_trap_handler (basic panic handler)
- Prints boot banner using SBI console
- Zeros BSS segment
- Parses device tree (finds memory, CPU type, UART, PLIC, initramfs)
- Initializes page allocator
- **Initializes kernel memory map** (creates proper page table, see below)
- Switches to high-memory kernel stack
- Calls kernel_main()

### kernel_memory_map::init() Details
This is a critical transition point where the real kernel page table is created:
- Creates new PageMapper with proper kernel mappings
- Identity maps available DRAM region (typically 0x80000000+, parsed from DTB)
- Maps kernel code/data/rodata to high memory with proper permissions
- Allocates and maps kernel heap (high memory, RW)
- Allocates and maps kernel stack (high memory, RW)
- **Hardware-specific MMIO mapping**:
  - QEMU: UART (0x1000_0000) and PLIC (0x0c00_0000-0x0c40_0000)
  - NanoRV: UART (0x0414_0000) and PLIC (0x7000_0000-0x7040_0000) with T-Head SO flags
- Maps process trampoline to high address
- Activates page table (csrw satp, sfence.vma)
- Initializes kernel heap allocator

**After this**: Heap available, MMIO accessible, proper kernel page table active

---

## Stage 3: kernel_main() - Kernel Initialization

**File**: `src/kernel_main.rs`
**CPU Mode**: S-mode

### State
- **Interrupts**: **DISABLED** (sstatus.SIE = 0, sie = 0)
- **Memory Map**: Kernel page table (high memory kernel, MMIO mapped)
- **Trap Handler**: kernel_trap_entry (kernel panic handler)
- **Stack**: High-memory kernel stack
- **Console**: **SBI console** (switches to kprintln! after init)
- **Heap**: Available

### What Happens
- Sets stvec to kernel_trap_entry
- Initializes PLIC and UART drivers
- **Mounts VFS**: `vfs::init(initramfs::new())`
  - Creates ramfs instance populated from CPIO initramfs
  - Registers root inode with VFS
  - Files accessible via `vfs_open()`, `vfs_read()`, etc.
- Initializes uart_writer (async message-based UART output thread)
- Initializes kprint (kernel print via uart_writer)
- Initializes idle thread
- Displays interactive boot menu
- On process selection: spawns user process as kernel thread, starts scheduler

---

## Stage 3.5: Scheduler and Kernel Threads

**File**: `src/thread.rs`
**CPU Mode**: S-mode

### How User Processes Start
User processes run as kernel threads via the scheduler:
1. `kthread::user_process::spawn_process(path)` creates a Thread with process context
2. Thread is added to scheduler queue
3. `thread::start_scheduler()` begins scheduling (never returns)
4. When the user process thread is scheduled, it calls `enter_process()`

### Kernel Thread Types
- **uart_writer**: Handles async UART output via message passing
- **idle**: Runs when no other threads are ready (executes `wfi`)
- **user_process**: Wraps a user process, calls enter_process()

---

## Stage 4: enter_process() - Transition to User Mode

**File**: `src/process_trampoline.rs`
**CPU Mode**: S-mode (until sret)

### State on Entry
- **Interrupts**: **DISABLED** (sstatus.SIE = 0, sie = 0)
- **Memory Map**: Kernel page table
- **Trap Handler**: kernel_trap_entry
- **Stack**: Kernel thread stack (with ProcessTrapFrame at top)
- **Console**: kprintln! available

### What Happens
- Sets current process context (global pointer)
- Sets up trampoline trap frame (kernel stack pointer, kernel satp)
- Enables supervisor interrupts in sie register (0x202 = SEIE | SSIE)
- Prepares sstatus for user mode (SPIE = 1, SPP = 0)
- **Changes stvec to trap_entry** (user-mode trap handler)
- Switches to process page table (csrw satp, sfence.vma)
- Sets sepc = user entry point, sp = user stack
- **Executes sret** → U-mode, interrupts enabled

### Critical Transitions
- stvec: kernel_trap_entry → trap_entry
- Page table: kernel → process
- CPU mode: S-mode → U-mode
- Interrupts: disabled → enabled (by sret)

---

## Stage 5: User Mode Execution

**CPU Mode**: U-mode

### State
- **Interrupts**: **ENABLED** (sstatus.SIE = 1, sie = 0x202)
- **Memory Map**: Process page table (user low, kernel/trampoline high)
- **Trap Handler**: trap_entry
- **Stack**: User stack
- **Console**: Not used yet

### What Happens
- User program executes normally
- Can make syscalls (ecall instruction)
- Can receive interrupts (UART, timer, etc.)
- Cannot access kernel memory (would page fault)

### When Trap Occurs (Syscall, Page Fault, Interrupt)
Hardware automatically:
1. Clears sstatus.SIE → 0 (**interrupts disabled**)
2. Sets scause, sepc, stval, sstatus.SPP, sstatus.SPIE
3. Jumps to stvec (trap_entry)

---

## Stage 6: trap_entry - User Trap Entry (Assembly)

**File**: `src/trap.rs` (assembly)
**CPU Mode**: S-mode
**Link Section**: `.process_trampoline` (mapped in both kernel and process page tables)

### State on Entry
- **Interrupts**: **DISABLED** (hardware cleared sstatus.SIE)
- **Memory Map**: Process page table (still active)
- **Stack**: User stack (sp points to user memory)
- **Trap Info**: scause, sepc, stval, sstatus saved by hardware

### What Happens
- Swaps sp with sscratch (get trampoline trap frame)
- Saves user sp and t0 to trampoline trap frame
- **Switches to kernel page table** (csrw satp, sfence.vma)
- Flushes caches (T-Head dcache/icache, standard sfence.vma)
- Switches to kernel stack (ProcessTrapFrame)
- Saves all registers (31 GPRs + trap CSRs) to ProcessTrapFrame
- Calls trap_handler() in Rust

---

## Stage 7: trap_handler() - Handle Trap (Rust)

**File**: `src/trap.rs` (Rust function)
**CPU Mode**: S-mode
**Link Section**: `.process_trampoline`

### State
- **Interrupts**: **DISABLED** (sstatus.SIE = 0)
- **Memory Map**: Kernel page table
- **Stack**: Kernel stack (ProcessTrapFrame)
- **Console**: **SBI console**
- **Heap**: Available

### What Happens
Dispatches based on scause:
- **AMO fault (7)**: Emulate atomic operation (T-Head C906 doesn't support AMO on MMIO)
- **Page fault (13, 15)**: Grow user stack or panic
- **User ecall (8)**: Handle syscall (write, brk, mmap)
- **External interrupt (0x8000000000000009)**: **Currently panics - TODO!**
- Other: Panic with diagnostic info

### Syscall Handling
- Dispatches based on a7 register (syscall number)
- Modifies trap frame (e.g., a0 = return value)
- Returns ProcessTrapFrame address

### Mutual Exclusion
**No locks needed!** All kernel code runs with interrupts disabled:
- Syscalls: Hardware disables interrupts on trap entry
- Interrupt handlers: Would run in trap_handler (already disabled)
- Kernel code: Never enables interrupts (sstatus.SIE always 0)

---

## Stage 8: trap_entry - User Trap Return (Assembly)

**File**: `src/trap.rs` (assembly, return path)
**CPU Mode**: S-mode

### State
- **Interrupts**: **DISABLED** (sstatus.SIE = 0)
- **Memory Map**: Kernel page table
- **Stack**: Kernel stack (ProcessTrapFrame)

### What Happens
- Restores all registers from ProcessTrapFrame
- Restores sepc and sstatus
- **Switches to process page table** (csrw satp, sfence.vma)
- Flushes caches (T-Head dcache/icache, standard sfence.vma)
- Restores final registers (t0, sp) from trampoline trap frame
- **Executes sret** → U-mode, interrupts re-enabled

### Critical Transitions
- Page table: kernel → process
- CPU mode: S-mode → U-mode
- Interrupts: disabled → enabled (by sret)
- PC: Resume at sepc (after ecall or faulting instruction)

User program continues execution...

---

## Stage 9: Kernel Panic (kernel_trap_entry)

**File**: `src/kernel_trap.rs`
**When**: Trap occurs while in kernel mode (S-mode)

### State
- **Interrupts**: **DISABLED** (hardware cleared sstatus.SIE)
- **Memory Map**: Kernel page table
- **Console**: SBI console works

### What Happens
- Loads dedicated kernel panic stack
- Calls kernel_trap_handler()
- Reads scause, sepc, stval
- Prints diagnostic info
- Enters infinite wfi loop (halt)

**No recovery** - fatal kernel error

### Why Separate from trap_handler?
- **kernel_trap_entry**: Handles kernel-mode traps (kernel bugs)
- **trap_entry**: Handles user-mode traps (syscalls, page faults, interrupts)
- Different stack setup, different purposes

---

## Console Implementation

### SBI Console (println!)

**Used in**:
- Early boot stages (rust_main, before uart_writer init)
- Panic handlers (always available fallback)

**How it works**:
- println! → PutcharWriter → sbi_console_putchar()
- sbi_console_putchar() performs ecall to M-mode firmware
- Firmware outputs to serial port

**Pros**: Simple, always available
**Cons**: Slow (ecall per character), blocks caller

### Kernel Console (kprintln!)

**Used in**:
- kernel_main and kernel threads (after uart_writer::init())
- Kernel diagnostics and debugging

**How it works**:
- kprintln! → kprint::CONSOLE (cached File) → vfs_write()
- vfs_write() → UartFileOps::write() → sends message to uart_writer thread
- uart_writer thread receives message, writes to UART

**Architecture**:
- Message-passing based (thread::send/receive)
- uart_writer is a dedicated kernel thread with elevated priority
- Non-blocking for callers (message queued, writer drains asynchronously)
- Uses UART TX with polling (interrupt-driven TX is future work)

### User Console (fd 1)

**Used in**:
- User processes via sys_write(1, buf, len)

**How it works**:
- Process fd 1 is initialized to uart_open() File
- sys_write translates user VA to PA, calls vfs_write()
- Same path as kprintln! from there

---

## Two Trap Handlers Summary

### kernel_trap_entry / kernel_trap_handler
- **File**: src/kernel_trap.rs
- **Active**: During kernel_main and kernel thread execution
- **Purpose**: Panic handler for kernel bugs
- **Stack**: Dedicated KERNEL_STACK (4 KB)
- **Behavior**: Print diagnostics, halt

### trap_entry / trap_handler
- **File**: src/trap.rs
- **Active**: After enter_process sets stvec, during all user execution
- **Purpose**: Handle user-mode traps (syscalls, page faults, interrupts)
- **Stack**: ProcessTrapFrame on kernel stack in process context
- **Behavior**: Dispatch to handler, return to user

### stvec Timeline

| Stage | stvec Value | Handler |
|-------|-------------|---------|
| M-mode firmware | mtvec | M-mode handler |
| _start trampoline | **Not set** | **None - DANGER ZONE!** |
| rust_main | boot_trap_handler | Minimal panic handler |
| kernel_main | kernel_trap_entry | Kernel panic handler |
| enter_process | trap_entry | User trap handler |
| User mode | trap_entry | User trap handler |

---

## Key Takeaways

1. **Interrupts only enabled in user mode**
   - Kernel always runs with sstatus.SIE = 0
   - Hardware disables interrupts on trap entry
   - Provides implicit mutual exclusion for kernel code

2. **Page table switching happens in trampoline code**
   - User PT → Kernel PT on trap entry
   - Kernel PT → User PT on trap return
   - Trampoline mapped in both page tables for safe switching

3. **Two separate trap handlers**
   - kernel_trap_entry: Kernel bugs (panic and halt)
   - trap_entry: User traps (handle and return)

4. **Boot process has critical danger zone**
   - _start runs with no trap handler (stvec not set)
   - Any bug would crash immediately
   - Code must be extremely careful

5. **VFS provides unified file access**
   - Root inode registered via vfs::init()
   - Path traversal via inode.lookup()
   - FileOps trait for read/write/seek/readdir
   - Inode trait for filesystem metadata and lookup

6. **Console has multiple implementations**
   - println! (SBI): Always available, slow, for early boot and panic
   - kprintln! (uart_writer): Message-based, non-blocking, for kernel threads
   - sys_write fd 1: User process output via same uart_writer path
