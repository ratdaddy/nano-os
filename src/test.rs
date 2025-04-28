#![allow(dead_code)]

pub fn test_runner(tests: &[&dyn Fn()]) {
    println!("Running {} tests...", tests.len());
    for test in tests {
        test();
    }
}

pub fn exit_qemu() -> ! {
    unsafe {
        core::arch::asm!(
            "li a7, 8", // SBI call for shutdown
            "ecall",
            options(noreturn)
        );
    }
}
