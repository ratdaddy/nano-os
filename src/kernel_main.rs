use core::sync::atomic::Ordering;

use crate::dtb;
use crate::initramfs;
use crate::io::Read;
use crate::plic;
use crate::process;
use crate::process_memory_map;
use crate::process_trampoline;
use crate::read_elf;
use crate::uart;

/*
extern "C" {
    pub fn trap_entry();
}
*/

pub fn kernel_main() {
    println!("In kernel_main");

    process::init();

    // Could reclaim pages used in original page map and early boot stack here

    unsafe {
        core::arch::asm!(
            "csrw stvec, {}",
            in(reg) kernel_trap_entry as usize,
        );
    }

    //uart_demo();

    /*
    test_stack_allocation();
    */

    let initrd_start = dtb::INITRD_START.load(Ordering::Relaxed);
    let initrd_len = dtb::INITRD_END.load(Ordering::Relaxed) - initrd_start;
    inspect_initramfs(initrd_start as *const u8);

    let slice = unsafe { core::slice::from_raw_parts(initrd_start as *const _, initrd_len) };
    initramfs::ifs_mount(slice);

    let mut handle = initramfs::ifs_open("/etc/motd").unwrap();
    let mut contents = alloc::string::String::new();
    let _result = handle.read_to_string(&mut contents);

    println!("Contents of /etc/motd: {}", contents);

    let mut handle = initramfs::ifs_open("/prog_example").unwrap();
    let header = read_elf::read_elf64_header(&mut handle).unwrap();
    println!("Reading ELF for /prog_example");
    println!("Entry point:     {:#x}", header.e_entry);
    println!("PH offset:       {:#x}", header.e_phoff);
    println!("PH entry size:   {}", header.e_phentsize);
    println!("PH count:        {}", header.e_phnum);

    let program_headers = read_elf::read_program_headers(&mut handle, &header).unwrap();

    for ph in &program_headers {
        println!("Program header: type: {:#x} offset: {:#x} virt addr:{:#x}-{:#x} file size: {:#x} mem size: {:#x}",
            ph.p_type, ph.p_offset, ph.p_vaddr, ph.p_vaddr + ph.p_memsz, ph.p_filesz, ph.p_memsz);
    }

    println!();

    let mut handle = initramfs::ifs_open("/prog_example").unwrap();

    println!();

    let context = process::create();

    process_memory_map::init_from_elf(&mut handle, context);

    println!("Process context initialized");

    unsafe {
        println!("Entering process trampoline");
        process_trampoline::enter_process(context);
    }

    #[allow(unreachable_code)]
    {
        println!("entering kernel wait loop");
        loop {
            unsafe { core::arch::asm!("wfi") }
        }
    }
}


pub fn uart_demo() {
    let uart = if dtb::get_cpu_type() == dtb::CpuType::LicheeRVNano {
        uart::Uart::new(uart::NANO_UART)
    } else {
        uart::Uart::new(uart::QEMU_UART)
    };

    println!("about to write to UART");
    uart.write_str("Direct write to uart\r\n");

    uart.enable_rx_interrupt();
    unsafe {
        plic::init();
    }

    println!("Enabling S-mode interrupts");
    unsafe {
        // Enable external, timer, and software interrupts in sie register
        // 0x222 = SEIE (bit 9) | STIE (bit 5) | SSIE (bit 1)
        core::arch::asm!(
            "li t0, 0x222",
            "csrw sie, t0",
        );

        // Enable interrupts globally in S-mode by setting SIE bit (bit 1) in sstatus
        core::arch::asm!(
            "li t0, (1 << 1)",
            "csrs sstatus, t0",
        );
    }
    println!("Interrupts enabled, waiting for input...");

    let plic_base = if dtb::get_cpu_type() == dtb::CpuType::LicheeRVNano {
        0x7000_0000usize
    } else {
        0x0c00_0000usize
    };
    let uart_base = if dtb::get_cpu_type() == dtb::CpuType::LicheeRVNano {
        0x0414_0000usize
    } else {
        0x1000_0000usize
    };
    let is_nano = dtb::get_cpu_type() == dtb::CpuType::LicheeRVNano;

    unsafe {
        // IRQ 44 is in pending register word 1 (offset 0x1004), bit 12
        let plic_pending0_initial = ((plic_base + 0x1000) as *const u32).read_volatile();
        let plic_pending1_initial = ((plic_base + 0x1004) as *const u32).read_volatile();

        // Check PLIC configuration
        // IRQ 44 is in enable word 1 (IRQs 32-63)
        let plic_enable_word1 = ((plic_base + 0x2084) as *const u32).read_volatile();
        let plic_threshold = ((plic_base + 0x201000) as *const u32).read_volatile();
        let uart_priority = ((plic_base + 0x2c * 4) as *const u32).read_volatile();

        // Check IRQ 37 (bit 25 in pending[0])
        let irq37_priority = ((plic_base + 37 * 4) as *const u32).read_volatile();

        println!("PLIC config: enable_word1={:#x} threshold={:#x} uart_priority={:#x}",
                 plic_enable_word1, plic_threshold, uart_priority);
        println!("IRQ 37 priority: {:#x}", irq37_priority);
        println!("Initial: pending[0]={:#x} pending[1]={:#x}",
                 plic_pending0_initial, plic_pending1_initial);

        // Check claim register - what IRQ is being delivered?
        let plic_claim = ((plic_base + 0x201004) as *const u32).read_volatile();
        println!("PLIC claim register: {:#x}", plic_claim);

        // If there's a pending claim, complete it
        if plic_claim != 0 {
            println!("Completing pending IRQ {}", plic_claim);
            ((plic_base + 0x201004) as *mut u32).write_volatile(plic_claim);

            // Re-check after completion
            let plic_claim_after = ((plic_base + 0x201004) as *const u32).read_volatile();
            println!("PLIC claim after completion: {:#x}", plic_claim_after);
        }

        println!("Waiting for interrupt (press any key)...");
        let mut count = 0;
        loop {
            unsafe {
                core::arch::asm!("wfi");
            }
            /*
                // Read PLIC pending registers (IRQ 44 is in word 1, bit 12)
                let plic_pending0 = ((plic_base + 0x1000) as *const u32).read_volatile();
                let plic_pending1 = ((plic_base + 0x1004) as *const u32).read_volatile();

                // Read UART LSR (Line Status Register) - offset 5
                let lsr_offset = if is_nano { 5 << 2 } else { 5 };
                let uart_lsr = ((uart_base + lsr_offset) as *const u32).read_volatile();

                // Read UART IIR (Interrupt Identification Register) - offset 2
                let iir_offset = if is_nano { 2 << 2 } else { 2 };
                let uart_iir = ((uart_base + iir_offset) as *const u32).read_volatile();

                // Read SIP (Supervisor Interrupt Pending)
                let sip: usize;
                core::arch::asm!("csrr {}, sip", out(reg) sip);

                // Print every 10000000 iterations or if something changed
                if count % 10000000 == 0 ||
                   plic_pending0 != plic_pending0_initial ||
                   plic_pending1 != plic_pending1_initial ||
                   (uart_lsr & 1) != 0 {
                    println!("PLIC_pend[0]={:#x} [1]={:#x} UART_LSR={:#x} IIR={:#x} SIP={:#x}",
                             plic_pending0, plic_pending1, uart_lsr, uart_iir, sip);
                }
            count += 1;
            */
        }
    }
}

pub fn inspect_initramfs(start: *const u8) {
    unsafe {
        // Read the first 6 bytes as the magic number
        let magic = core::str::from_utf8_unchecked(core::slice::from_raw_parts(start, 6));
        if magic != "070701" {
            println!("Invalid cpio magic: {}", magic);
            return;
        }
        println!("CPIO magic: {}", magic);

        // Grab more interesting fields
        let namesize_str =
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(start.add(94), 8));
        let mode_str =
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(start.add(14), 8));
        let filesize_str =
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(start.add(102), 8));

        let namesize = usize::from_str_radix(namesize_str, 16).unwrap_or(0);
        let mode = u32::from_str_radix(mode_str, 16).unwrap_or(0);
        let filesize = usize::from_str_radix(filesize_str, 16).unwrap_or(0);

        println!("Mode: {:o}", mode);
        println!("File size: {} bytes", filesize);
        println!("Name size: {} bytes", namesize);

        // Optionally print the filename too
        let name_start = start.add(110);
        let name_bytes = core::slice::from_raw_parts(name_start, namesize);
        if let Ok(name) = core::str::from_utf8(name_bytes) {
            println!("Filename: {}", name.trim_end_matches('\0'));
        }
    }
}

#[allow(dead_code)]
fn test_stack_allocation() {
    let data = [42u8; 10 * 1024];

    // Touch the memory so it’s not optimized out
    let mut sum = 0u32;
    for &byte in &data {
        sum += byte as u32;
    }

    // Pass by value to copy onto the callee's stack
    consume_array(data);

    // Use result so the compiler doesn't optimize everything away
    println!("Sum: {}", sum);
}

#[allow(dead_code)]
fn consume_array(arr: [u8; 10 * 1024]) {
    let avg = arr.iter().map(|&b| b as u32).sum::<u32>() / arr.len() as u32;
    println!("Average: {}", avg);
}

#[repr(align(16))]
struct AlignedStack([u8; 4096]);

#[no_mangle]
static mut KERNEL_STACK: AlignedStack = AlignedStack([0; 4096]);

extern "C" {
    pub fn kernel_trap_entry();
}

core::arch::global_asm!(
    ".section .text.trap_entry",
    ".globl kernel_trap_handler",
    ".type trap_entry_panic, @function",
    "kernel_trap_entry:",
    "la sp, KERNEL_STACK + 4096",
    "call kernel_trap_handler",
);

#[no_mangle]
pub extern "C" fn kernel_trap_handler() {
    let scause: usize;
    let sepc: usize;
    let stval: usize;

    unsafe {
        core::arch::asm!(
            "csrr {0}, scause",
            "csrr {1}, sepc",
            "csrr {2}, stval",
            out(reg) scause,
            out(reg) sepc,
            out(reg) stval,
        );
    }

    println!("*** KERNEL TRAP ***");
    println!("scause = {:#x}", scause);
    println!("sepc   = {:#x}", sepc);
    println!("stval  = {:#x}", stval);

    // Halt the system
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}
