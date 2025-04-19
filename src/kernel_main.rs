pub fn kernel_main() {
    println!("In kernel_main");

    // Could reclaim pages used in original page map and early boot stack here

    loop {
        unsafe { core::arch::asm!("wfi") }
    }
}
