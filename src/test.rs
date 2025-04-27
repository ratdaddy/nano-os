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

#[cfg(test)]
mod tests {
    #[test_case]
    fn test_addition() {
        assert_eq!(2 + 2, 4);
    }

    #[test_case]
    fn test_something_hard() {
        assert!(1 == 2);
    }

    fn test_another_failure () {
        assert!(1 == 2);
    }
}
