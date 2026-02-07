#![allow(dead_code)]
#![cfg(test)]
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
use core::cell::UnsafeCell;

pub fn test_runner(tests: &[&dyn Fn()]) {
    init_test_alloc();
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

pub struct TestBumpAllocator {
    heap_start: UnsafeCell<*mut u8>,
    heap_end: *mut u8,
}

unsafe impl Sync for TestBumpAllocator {}

impl TestBumpAllocator {
    pub const fn new() -> Self {
        Self {
            heap_start: UnsafeCell::new(null_mut()),
            heap_end: null_mut(),
        }
    }

    pub unsafe fn init(&mut self, heap_start: *mut u8, heap_size: usize) {
        *self.heap_start.get() = heap_start;
        self.heap_end = heap_start.add(heap_size);
    }
}

unsafe impl GlobalAlloc for TestBumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = layout.align();
        let size = layout.size();

        let start = *self.heap_start.get();
        let aligned = start.add(start.align_offset(align));

        if aligned.add(size) > self.heap_end {
            null_mut()
        } else {
            *self.heap_start.get() = aligned.add(size);
            aligned
        }
    }

    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {
        // no-op (bump allocator)
    }
}

#[global_allocator]
static mut TEST_ALLOCATOR: TestBumpAllocator = TestBumpAllocator::new();

static mut TEST_HEAP: [u8; 32 * 1024] = [0; 32 * 1024]; // 32 KiB heap

#[no_mangle]
pub extern "C" fn init_test_alloc() {
    unsafe {
        (*core::ptr::addr_of_mut!(TEST_ALLOCATOR)).init(
            core::ptr::addr_of_mut!(TEST_HEAP) as *mut u8,
            32 * 1024,
        );
    }
}
