use core::arch::naked_asm;

#[link_section = ".tramp_data"]
#[no_mangle]
pub static mut _STACK_TOP: [u8; 4096 * 16] = [0; 4096 * 16];

#[link_section = ".tramp_data"]
#[no_mangle]
pub static mut _ROOT_PAGE_TABLE: [u64; 512] = [0; 512];

#[naked]
#[no_mangle]
#[link_section = ".text.trampoline"]
pub extern "C" fn _start() -> ! {
    unsafe {
        naked_asm!(
            // Save hartid and dtb_ptr
            "mv s0, a0",
            "mv s1, a1",

            // Set stack pointer
            "la sp, _STACK_TOP + (4096 * 16)",

            // Clear root page table
            "la t0, _ROOT_PAGE_TABLE",
            "li t1, 0",
            "li t2, 512",
            "0:",
            "sd t1, 0(t0)",
            "addi t0, t0, 8",
            "addi t2, t2, -1",
            "bnez t2, 0b",

            // Set up PTE (identity and high-half mapping)
            "li t3, 0x80000",         // 0x80000000 >> 12
            "slli t3, t3, 10",
            "li t4, 0x0CF",           // V|R|W|X|A|D
            "or t3, t3, t4",

            // ROOT_PAGE_TABLE[2] = t3
            "la t0, _ROOT_PAGE_TABLE",
            "sd t3, 16(t0)",          // 2 * 8

            // ROOT_PAGE_TABLE[510] = t3
            "li t5, 510",
            "slli t5, t5, 3",
            "add t5, t5, t0",
            "sd t3, 0(t5)",

            // Build SATP: (8 << 60) | (ROOT_PAGE_TABLE >> 12)
            "la t6, _ROOT_PAGE_TABLE",
            "srli t6, t6, 12",

            "li t5, 1",
            "slli t5, t5, 63",        // t5 = 1 << 63 (mode = 8)
            "or t6, t6, t5",

            "csrw satp, t6",
            "sfence.vma",

            // Restore arguments
            "mv a0, s0",
            "mv a1, s1",

            // Pass along kernel phys mem start and end addresses
            "la a2, _start",
            "la a3, _kernel_phys_end",

            // Jump to rust_main
            "la t0, rust_main",
            "jr t0",
        );
    }
}

