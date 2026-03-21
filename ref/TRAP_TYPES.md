# RISC-V Trap Types

This file documents all standard RISC-V trap causes, including both **exceptions** and **interrupts**, as defined in the RISC-V Privileged Spec (v1.12).

---

## Trap Codes

### Exception Codes

| Code | Name                           | Description                                        |
| ---- | ------------------------------ | -------------------------------------------------- |
| 0    | Instruction address misaligned | PC not aligned to instruction size.                |
| 1    | Instruction access fault       | Instruction fetch failed (e.g., MMU or bus error). |
| 2    | Illegal instruction            | Invalid opcode or unsupported instruction.         |
| 3    | Breakpoint                     | Triggered by `ebreak` instruction (debug).         |
| 4    | Load address misaligned        | Load address not aligned to type size.             |
| 5    | Load access fault              | Failed memory load (e.g., MMU or bus error).       |
| 6    | Store/AMO address misaligned   | Store or AMO address misaligned.                   |
| 7    | Store/AMO access fault         | Failed store or atomic memory operation.           |
| 8    | Environment call from U-mode   | `ecall` instruction from User mode.                |
| 9    | Environment call from S-mode   | `ecall` instruction from Supervisor mode.          |
| 11   | Environment call from M-mode   | `ecall` from Machine mode.                         |
| 12   | Instruction page fault         | Page fault on instruction fetch.                   |
| 13   | Load page fault                | Page fault on load.                                |
| 15   | Store/AMO page fault           | Page fault on store or AMO.                        |

### Interrupt Codes

| Code | Name                          | Description                               |
| ---- | ----------------------------- | ----------------------------------------- |
| 0    | User software interrupt       | Software interrupt (triggered by `msip`). |
| 1    | Supervisor software interrupt | Typically used for inter-core signaling.  |
| 3    | Machine software interrupt    | Highest privilege software interrupt.     |
| 4    | User timer interrupt          | Timer interrupt in user mode.             |
| 5    | Supervisor timer interrupt    | S-mode timer (e.g., `mtimecmp`).          |
| 7    | Machine timer interrupt       | M-mode timer.                             |
| 8    | User external interrupt       | Device-level interrupt in U-mode.         |
| 9    | Supervisor external interrupt | Usually routed from PLIC to S-mode.       |
| 11   | Machine external interrupt    | Routed from PLIC to M-mode.               |

---

## Trap Encoding

Trap causes are encoded in the `mcause`, `scause`, or `ucause` CSR:

* If the **most significant bit** is `0`: it's an **exception**
* If the **most significant bit** is `1`: it's an **interrupt**

To extract the cause number:

```c
cause_num = cause & ~(1 << (XLEN - 1));
is_interrupt = (cause >> (XLEN - 1)) != 0;
```

---

## Notes

* Codes `10`, `14`, and other undefined numbers are **reserved**.
* Trap vectors (`stvec`, `mtvec`) should be prepared to handle unexpected causes gracefully.
* Most systems use **delegation** to route U/S-mode traps via `medeleg`/`mideleg`.

---

## References

* RISC-V Privileged Spec v1.12 ([link](https://github.com/riscv/riscv-isa-manual/releases))
* `mcause`, `scause`, `ucause` CSRs
* `stvec`, `mtvec`, `sepc`, `mepc` trap CSRs