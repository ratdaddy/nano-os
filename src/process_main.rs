#[no_mangle]
#[link_section = ".process_main"]
pub fn process_main() {
    unsafe {
        core::arch::asm!(
            "ecall"
        );
    }
    loop {
        unsafe { core::arch::asm!("wfi") }
    }
}