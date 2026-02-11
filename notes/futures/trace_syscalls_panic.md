# trace_syscalls Panic Investigation

## Symptom

Running a single user process panics **only** when `trace_syscalls` feature is enabled:

```
[read]: fd: 3, buf: 0xffdffPanic: panicked at src/trap.rs:180:17:
Unhandled exception: scause=0xc, stval=0x0, sepc=0x0
```

- scause=0xc = instruction page fault
- sepc=0x0 = CPU tried to execute at address 0
- Happens consistently on the second `read` syscall
- Output gets cut off mid-string (`0xffdff` instead of full address)
- Works fine with all features off

## What We Ruled Out

1. **Stack overflow** — Increased thread stack from 16KB to 24KB, didn't help. Reverted.
2. **File drop in close** — Added sync `println!` before/after `*slot = None` in close handler. Close completes fine. (Debug prints removed.)

## Key Architectural Finding: kprintln! Yields During Syscall Handling

Every `kprintln!` call causes a **context switch**:

```
kprintln!("...")
  → KPrintWriter formats string into buffer
  → Drop calls vfs_write(console(), buf)
    → UartFileOps::write
      → uart_writer::send_write(buf)
        → send_message_urgent()  ← naked fn, saves context, YIELDS
```

This means during a single syscall, the process thread yields multiple times (once per kprintln!). With `trace_syscalls` off, there are zero yields during syscall handling.

## The Suspect: stvec Points to Process Trampoline During Kernel Execution

When `schedule()` resumes a user process thread, it sets:
- `stvec = trap::trap_entry` (the **process** trampoline)
- `kernel_sp = process_ctx.trap_frame` address in TrampolineTrapFrame

This happens **before** restoring the kernel thread context. So after the context switch, the thread resumes executing kernel code (middle of syscall handler) but stvec points to the process trampoline.

### What happens if an interrupt fires during kernel-mode syscall handling?

The process trampoline was designed for **user→kernel** traps. If a **kernel→kernel** trap occurs:

1. `csrrw sp, sscratch, sp` — swaps kernel sp with TrampolineTrapFrame pointer
2. Saves kernel register values as "user" registers to TrampolineTrapFrame
3. Loads `kernel_sp` (= PTF address) into sp
4. **OVERWRITES ProcessTrapFrame user registers with kernel register values**
5. **OVERWRITES PTF.sepc with kernel instruction address**
6. **OVERWRITES PTF.sstatus** (now has SPP=1 for S-mode, not SPP=0 for U-mode)
7. Calls trap_handler, handles interrupt, returns
8. Return path loads PTF.pc → sepc, restores corrupted PTF registers, does sret

After this, the ProcessTrapFrame is corrupted — it has a mix of kernel and user values. The next sret back to user mode would use corrupted state.

### PTF.pc vs PTF.sepc — separate fields!

```rust
pub struct ProcessTrapFrame {
    pub registers: GpRegisters,  // 31 regs
    pub pc: usize,               // used by sret return path
    pub sepc: usize,             // saved by trap entry
    ...
}
```

- Trap entry saves hardware sepc → `PTF.sepc`
- Trap return loads `PTF.pc` → hardware sepc for sret
- `syscall::handle()` sets `tf.pc = tf.sepc + 4` at the **END** (line 46 of syscall/mod.rs)

So during a kprintln! yield (mid-syscall), PTF.pc still has the value from the **previous** syscall return. If PTF gets overwritten by a kernel-mode interrupt going through the process trampoline, PTF.pc could end up as 0 (if it was zero-initialized or corrupted).

## Interrupt Enable State

- After ecall trap entry: `sstatus.SIE = 0` (interrupts disabled in S-mode)
- The idle thread is the only place that enables `sstatus.SIE`
- Kernel threads and syscall handlers do NOT explicitly enable/disable SIE
- **Key question**: When schedule() context-switches between threads, what is SIE? The idle thread does `csrc sstatus, SIE` before calling `schedule_if_ready()`. The kernel trap handler clears SPIE before sret (line 98-102 of kernel_trap.rs). So SIE should generally be 0 during kernel execution.

**However**: If the UART writer thread gets an interrupt while blocked in `push_slice_blocking` and the kernel trap handler returns with sret, the sequence is:
- Trap entry: SIE→SPIE, SIE=0
- Kernel trap handler clears SPIE
- sret: SIE = SPIE = 0

So interrupts should stay disabled. **But** — need to verify this on the actual hardware (T-Head C906). The C906 may have quirks.

## Ring Buffer Hypothesis (User's Latest Suggestion)

TX_RING_SIZE is only 1024 bytes. With `trace_syscalls`, every syscall generates 50-100 bytes of output. The ring buffer drains at UART baud rate (~11.5 KB/s at 115200). Under heavy syscall load, the ring could fill up fast.

When full, `push_slice_blocking()` in the UART writer thread:
1. Enables TX interrupt
2. Sets WAITING_FOR_SPACE = true
3. Calls `block_now()` which yields

This blocks the **UART writer thread**, not the process thread. The process thread just sends messages. But if the writer's inbox fills with messages while it's blocked waiting for ring space, there could be excessive memory allocation (each message is a `Box<WriterMessage>` containing a `Vec<u8>`).

This doesn't directly explain sepc=0x0, but could contribute to memory pressure or timing that triggers the stvec bug above.

## Current Workaround

Switched all `trace_syscalls`-gated output from `kprintln!` (async, yields) to `println!` (sync, direct UART). This avoids mid-syscall context switches and **fixes the panic**. The workaround confirms the root cause is related to yielding during syscall handling, not the ring buffer.

## Future: Proper Fix for kprintln! During Syscall Handling

If we ever want async `kprintln!` to work during syscall handling, the underlying stvec problem must be fixed. Options:

1. **Verify the stvec theory**: Add a check at the start of `trap_handler` in trap.rs — if `sstatus.SPP == 1` (trapped from S-mode), print a diagnostic and halt instead of corrupting PTF. This would confirm whether kernel-mode traps are going through the process trampoline.

2. **Check SIE state**: Add `csrr` of sstatus after each `schedule()` resume point to verify SIE is actually 0. The C906 might behave differently.

3. **Best fix**: When `schedule()` resumes a user process thread, set `stvec` to the **kernel** trap handler initially, and only switch to the process trampoline in the trap return path (just before sret to user mode). This way, any interrupt during kernel-mode execution on a user process thread would be handled correctly by the kernel trap handler.

4. **Alternative**: Keep trace output synchronous (`println!`) and reserve `kprintln!` for non-syscall contexts where yielding is safe.

## Relevant Files

| File | Role |
|------|------|
| `src/trap.rs` | Process trampoline asm + trap_handler |
| `src/thread.rs` | schedule(), send_message_urgent, context switch |
| `src/syscall/mod.rs:46` | `tf.pc = tf.sepc + 4` — only at END of handle() |
| `src/kprint.rs` | KPrintWriter — yields on drop via vfs_write |
| `src/kthread/uart_writer.rs` | send_write, ring buffer, push_slice_blocking |
| `src/kernel_trap.rs` | Kernel trap handler (correct one for S-mode traps) |
| `src/process_trampoline.rs` | enter_process — initial sret to user mode |
| `src/kthread/idle.rs` | Only place SIE is enabled |
| `types/src/lib.rs` | ProcessTrapFrame struct (pc and sepc are separate fields) |
