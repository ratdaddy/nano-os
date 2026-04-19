#![allow(dead_code)]
#![cfg(test)]

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ptr::{addr_of_mut, slice_from_raw_parts_mut};
use spin::Mutex;

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
    free_leaked();
    assert!(!TEST_ALLOCATOR.has_allocated_blocks(), "test leaked memory");
}

#[derive(Debug)]
struct AnyLeak {
    ptr: *mut u8,
    len: usize,
    drop_fn: unsafe fn(*mut u8, usize),
}
// SAFETY: tests are single-threaded; the Mutex provides exclusive access.
unsafe impl Send for AnyLeak {}

static CLEANUPS: Mutex<Vec<AnyLeak>> = Mutex::new(Vec::new());

/// Track a byte-slice allocation produced by `Box::into_raw` for cleanup.
pub fn register_leak(ptr: *mut u8, len: usize) {
    // SAFETY: ptr was produced by Box::into_raw with the matching len.
    unsafe fn drop_slice(ptr: *mut u8, len: usize) {
        drop(Box::from_raw(slice_from_raw_parts_mut(ptr, len)));
    }
    CLEANUPS.lock().push(AnyLeak { ptr, len, drop_fn: drop_slice });
}

/// Leak a typed `Box`, register it for cleanup, and return a `&'static` reference.
/// Entries are freed in LIFO order so dependents (e.g. a RamfsSuperBlock holding
/// Arc<Inode> into a Ramfs) are freed before the values they reference.
pub fn register_typed_leak<T: 'static>(val: Box<T>) -> &'static T {
    // SAFETY: ptr was produced by Box::into_raw for a T.
    unsafe fn drop_typed<T>(ptr: *mut u8, _: usize) {
        drop(Box::from_raw(ptr as *mut T));
    }
    let ptr = Box::into_raw(val);
    CLEANUPS.lock().push(AnyLeak { ptr: ptr as *mut u8, len: 0, drop_fn: drop_typed::<T> });
    // SAFETY: ptr is non-null and valid for 'static — ownership transferred to CLEANUPS.
    unsafe { &*ptr }
}

fn free_leaked() {
    let mut cleanups = CLEANUPS.lock();
    // SAFETY: each drop_fn matches the allocation made by register_leak / register_typed_leak.
    for AnyLeak { ptr, len, drop_fn } in cleanups.drain(..).rev() {
        unsafe { drop_fn(ptr, len); }
    }
    cleanups.shrink_to_fit();
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
