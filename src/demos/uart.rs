use crate::dtb;
use crate::drivers::uart;
use crate::kernel_trap;
use crate::riscv;

#[allow(dead_code)]
pub fn uart_demo() {
    // Enable UART RX interrupt for this demo
    uart::get().enable_rx_interrupt();

    println!("about to write to UART");
    uart::get().write_str("Direct write to uart\r\n");

    println!("Enabling S-mode interrupts");
    unsafe {
        // Set up sscratch with the trap stack address before enabling interrupts.
        // The kernel trap handler swaps sp with sscratch on entry.
        let trap_stack = kernel_trap::trap_stack_top();
        core::arch::asm!("csrw sscratch, {}", in(reg) trap_stack);

        // Enable all S-mode interrupt sources in sie register
        core::arch::asm!("csrw sie, {}", in(reg) riscv::SIE_ALL);

        // Enable interrupts globally in S-mode
        core::arch::asm!("csrs sstatus, {}", in(reg) riscv::SSTATUS_SIE);
    }
    println!("Interrupts enabled, waiting for input...");

    let plic_base = if dtb::get_cpu_type() == dtb::CpuType::LicheeRVNano {
        0x7000_0000usize
    } else {
        0x0c00_0000usize
    };

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
        loop {
            // Interrupt handler returns with interrupts disabled (SPIE cleared).
            // Re-enable before wfi, disable after to match handler expectations.
            core::arch::asm!(
                "csrs sstatus, {sie}",  // Enable interrupts
                "wfi",                   // Wait for interrupt
                "csrc sstatus, {sie}",  // Disable after waking
                sie = in(reg) riscv::SSTATUS_SIE,
            );
        }
    }
}
