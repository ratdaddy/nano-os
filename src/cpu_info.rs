//! CPU information and clock speed measurement

use crate::dtb;

// Timebase frequency from DTB (25 MHz for NanoRV)
const TIMEBASE_FREQUENCY: u64 = 25_000_000;

/// Read the RISC-V time CSR (counts at timebase frequency)
#[inline]
fn read_time() -> u64 {
    let time: u64;
    unsafe {
        core::arch::asm!("rdtime {}", out(reg) time);
    }
    time
}

/// Read the RISC-V cycle CSR (counts at CPU frequency)
#[inline]
fn read_cycle() -> u64 {
    let cycle: u64;
    unsafe {
        core::arch::asm!("rdcycle {}", out(reg) cycle);
    }
    cycle
}

/// Measure CPU clock frequency by comparing cycle counter to time counter
/// over a known timebase duration (0.1 seconds).
fn measure_cpu_frequency() -> u64 {
    let start_time = read_time();
    let start_cycle = read_cycle();

    // Wait 0.1 seconds (2,500,000 timebase cycles at 25 MHz)
    let duration = TIMEBASE_FREQUENCY / 10;
    let target = start_time + duration;

    while read_time() < target {
        // Busy-wait
    }

    let elapsed_time = read_time() - start_time;
    let elapsed_cycles = read_cycle() - start_cycle;

    // Calculate CPU frequency: cycles * timebase_freq / time_elapsed
    (elapsed_cycles * TIMEBASE_FREQUENCY) / elapsed_time
}

/// Display CPU information including clock speed measurement
pub fn show_cpu_info() {
    let cpu_type = match dtb::get_cpu_type() {
        dtb::CpuType::Qemu => "QEMU virt",
        dtb::CpuType::LicheeRVNano => "LicheeRV Nano (CV181x)",
        dtb::CpuType::Unknown => "Unknown",
    };
    println!("Platform: {}", cpu_type);
    println!("Timebase frequency: {} MHz", TIMEBASE_FREQUENCY / 1_000_000);

    let cpu_freq = measure_cpu_frequency();
    let cpu_mhz = cpu_freq / 1_000_000;
    let multiplier = cpu_freq / TIMEBASE_FREQUENCY;

    println!("CPU frequency: {} MHz ({} Hz)", cpu_mhz, cpu_freq);
    println!("CPU/timebase ratio: {}x", multiplier);
}
