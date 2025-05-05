#[cfg(test)]
mod tests;

mod block_header;
mod allocator;

use allocator::LinkedListAllocator;

#[global_allocator]
pub static ALLOCATOR: LinkedListAllocator = LinkedListAllocator::new();

#[inline]
fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}


