use alloc::vec::Vec;
use alloc::vec;
use alloc::boxed::Box;

use crate::kernel_allocator;

extern "C" {
    fn trap_entry();
}

pub fn kernel_main() {
    println!("In kernel_main");

    // Could reclaim pages used in original page map and early boot stack here

    unsafe {
        core::arch::asm!(
            "csrw stvec, {}",
            in(reg) trap_entry as usize,
        );
    }

    test_stack_allocation();

    test_alloc1();
    test_alloc2();

    loop {
        unsafe { core::arch::asm!("wfi") }
    }
}

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

fn consume_array(arr: [u8; 10 * 1024]) {
    let avg = arr.iter().map(|&b| b as u32).sum::<u32>() / arr.len() as u32;
    println!("Average: {}", avg);
}

fn test_alloc1() {
    println!("\n*** Testing allocation ***");
    unsafe { kernel_allocator::ALLOCATOR.dump_heap(); }
    let _buffer1: Box<[u8]> = vec![0u8; 128].into_boxed_slice();
    let mut v = Vec::new();
    v.push(42);
    unsafe { kernel_allocator::ALLOCATOR.dump_heap(); }
    v.push(100);
    v.push(200);
    v.push(300);
    v.push(300);
    unsafe { kernel_allocator::ALLOCATOR.dump_heap(); }
}

fn test_alloc2() {
    let mut v = Vec::new();
    v.push(42);
    unsafe { kernel_allocator::ALLOCATOR.dump_heap(); }
    let _buffer1: Box<[u8]> = vec![0u8; 16000].into_boxed_slice();
    let _buffer2: Box<[u8]> = vec![0u8; 4016].into_boxed_slice();
    let _buffer3: Box<[u8]> = vec![0u8; 128].into_boxed_slice();
    unsafe { kernel_allocator::ALLOCATOR.dump_heap(); }
}
