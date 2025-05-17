OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS {
  /* ------------------------ */
  /* PHYSICAL BOOTSTRAP AREA  */
  /* ------------------------ */

  /* Execution begins here */
  . = 0x80200000;

  .text.trampoline : ALIGN(4K) {
    _start = .;
    KEEP(*(.text.trampoline))
  }

  .tramp_data : ALIGN(4K) {
    KEEP(*(.data.trampoline))
  }

  /* Compute the next free aligned physical address */
  __trampoline_phys_end = ALIGN(4K);

  /* ------------------------ */
  /* KERNEL VIRTUAL REGION    */
  /* ------------------------ */

  /* Start kernel virtual address just after trampoline offset */
  __high_half_phys = 0xffffffff00000000;
  __kernel_phys_start = __trampoline_phys_end;
  __kernel_virt_start = __high_half_phys + __kernel_phys_start;

  . = __kernel_virt_start;

  .text : AT(__kernel_phys_start) ALIGN(4K) {
    _text_start = .;
    *(.text .text.*)
    _text_end = .;
  }

  . = ALIGN(4K);

  .rodata : {
    _rodata_start = .;
    *(.rodata .rodata.* .srodata .srodata.*)
    _proc_tramp_lma = .;
    QUAD(_proc_tramp_phys_start)
    _proc_main_lma = .;
    QUAD(LOADADDR(.process_main))
    _rodata_end = .;
  }

  . = ALIGN(4K);

  .data : {
    _data_start = .;
    *(.data .data.* .sdata .sdata.* .data.rel.ro .data.rel.ro.*)
    _data_end = .;
  }

  . = ALIGN(4K);

  .bss : {
    _bss_start = .;
    *(.bss .bss.* COMMON .sbss .sbss.* .scommon)
    _bss_end = .;
  }

  . = ALIGN(4K);

  _kernel_end = .;
  _kernel_phys_end = _kernel_end - 0xffffffff00000000;

  . = 0xfffffffffffe0000;

  _proc_tramp_phys_start = _kernel_phys_end;

  .process_trampoline : AT(_kernel_phys_end) {
    _proc_tramp_start = .;
    *(.process_trampoline);
  }

  . = ALIGN(4K);

  _proc_tramp_end = .;
  _proc_tramp_phys_end = _proc_tramp_phys_start + (_proc_tramp_end - _proc_tramp_start);

 . = 0x10000;

 .process_main : AT(_proc_tramp_phys_end)  {
    _process_main = .;
    *(.process_main)
 }

 . = ALIGN(4K);

 _process_main_end = .;

  _image_phys_end = (_kernel_end - 0xffffffff00000000) + (_proc_tramp_end - _proc_tramp_start) + (_process_main_end - _process_main);
}
