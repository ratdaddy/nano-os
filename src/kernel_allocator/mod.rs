#[cfg(test)]
mod tests;

mod allocator;
mod block_header;

use allocator::LinkedListAllocator;

#[cfg(not(test))]
#[global_allocator]
pub static ALLOCATOR: LinkedListAllocator = LinkedListAllocator::new();

#[inline]
fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}
