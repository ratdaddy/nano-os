use crate::dtb;
use crate::drivers::plic;
use crate::drivers::uart;

#[allow(dead_code)]
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
            core::arch::asm!("wfi");
        }
    }
}
