OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS {
  . = 0x80200000;

  .text : ALIGN(4K) {
    *(.text._start)
    *(.text*)
  }

  .rodata : ALIGN(4K) {
    *(.rodata*)
  }

  .data : ALIGN(4K) {
    *(.data*)
  }

  .bss : ALIGN(4K) {
    *(.bss*)
    *(COMMON)
  }

  . = ALIGN(4K);
  _stack_end = .;
  . = . + 16K;
  _stack_start = .;
}
