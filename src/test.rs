#![allow(dead_code)]
#![cfg(test)]

use core::ptr::addr_of_mut;

use crate::dev::block::blkdev_clear;
use crate::dev::char::chrdev_clear;
use crate::vfs::vfs_clear;
use crate::kernel_allocator::LinkedListAllocator;

pub fn test_runner(tests: &[&dyn Fn()]) {
    init_test_alloc();
    println!("Running {} tests...", tests.len());
    for test in tests {
        test();
        test_cleanup();
    }
}

fn test_cleanup() {
    vfs_clear();
    blkdev_clear();
    chrdev_clear();
    assert!(!TEST_ALLOCATOR.has_allocated_blocks(), "test leaked memory");
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

#[global_allocator]
static TEST_ALLOCATOR: LinkedListAllocator = LinkedListAllocator::new_fixed();

const TEST_HEAP_SIZE: usize = 32 * 1024;

#[derive(Debug)]
#[repr(align(8))]
struct AlignedHeap([u8; TEST_HEAP_SIZE]);

static mut TEST_HEAP: AlignedHeap = AlignedHeap([0; TEST_HEAP_SIZE]);

#[no_mangle]
pub extern "C" fn init_test_alloc() {
    // SAFETY: TEST_HEAP is a static buffer not yet in use; called once before any allocation.
    unsafe {
        TEST_ALLOCATOR.init(
            addr_of_mut!(TEST_HEAP.0) as usize,
            TEST_HEAP_SIZE,
        );
    }
}
