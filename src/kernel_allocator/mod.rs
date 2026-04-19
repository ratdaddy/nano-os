#[cfg(test)]
mod tests;

mod allocator;
mod block_header;

use alloc::alloc::{alloc_zeroed, Layout};
use alloc::boxed::Box;
use core::mem::size_of;

use crate::memory::PAGE_SIZE;

#[cfg(not(test))]
use allocator::LinkedListAllocator;

#[cfg(test)]
pub use allocator::LinkedListAllocator;

#[cfg(not(test))]
#[global_allocator]
pub static ALLOCATOR: LinkedListAllocator = LinkedListAllocator::new();

#[inline]
fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

/// Allocate a zeroed box guaranteed to fit within a single physical page.
///
/// Hardware-shared buffers (DMA targets, virtqueue ring structures, descriptor
/// tables) must have contiguous physical addresses. The allocator guarantees
/// physical contiguity within a page but not across page boundaries. Aligning
/// to the next power-of-two >= size ensures the allocation cannot straddle a
/// page boundary, keeping its physical addresses contiguous.
///
/// See `ref/hardware-shared-buffer-alignment.md` for the full reasoning.
///
/// # Safety
///
/// `alloc_zeroed` returns a valid, non-null pointer for any non-zero layout on
/// this kernel's allocator. `Box::from_raw` is safe here because the kernel
/// allocator ignores `Layout` on deallocation, so the Box drop will not
/// misinterpret the allocation.
pub fn alloc_within_page<T>() -> Box<T> {
    let size = size_of::<T>();
    assert!(size <= PAGE_SIZE, "alloc_within_page: size {} exceeds page size", size);
    let align = size.next_power_of_two().min(PAGE_SIZE);
    let layout = Layout::from_size_align(size, align).unwrap();
    // SAFETY: alloc_zeroed returns a valid non-null pointer for any non-zero layout on this
    // allocator; Box::from_raw is sound because the kernel allocator ignores Layout on dealloc.
    unsafe {
        let ptr = alloc_zeroed(layout) as *mut T;
        Box::from_raw(ptr)
    }
}
