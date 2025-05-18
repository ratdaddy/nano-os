use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::null_mut;

use super::align_up;
use super::block_header::{BlockHeader, BLOCK_HEADER_SIZE};
use crate::kernel_memory_map;
use crate::memory;

pub const MIN_ALLOC_SIZE: usize = 16;
pub const MIN_BLOCK_SIZE: usize = BLOCK_HEADER_SIZE + MIN_ALLOC_SIZE;

unsafe impl GlobalAlloc for LinkedListAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if let Some((block, aligned_block)) = self.find_fit(layout) {
            self.split_block(block, aligned_block, layout)
        } else {
            null_mut()
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if ptr.is_null() {
            return;
        }

        self.dealloc_and_coalesce(ptr as *mut BlockHeader);
    }
}

pub struct LinkedListAllocator {
    pub head: UnsafeCell<*mut BlockHeader>,
    pub tail: UnsafeCell<*mut BlockHeader>,
    pub heap_end: UnsafeCell<*mut BlockHeader>,
    pub free_head: UnsafeCell<*mut BlockHeader>,
    pub grow_heap_fn: fn(usize) -> Option<(usize, usize)>,
}

impl LinkedListAllocator {
    pub const fn new() -> Self {
        LinkedListAllocator {
            head: UnsafeCell::new(null_mut()),
            tail: UnsafeCell::new(null_mut()),
            heap_end: UnsafeCell::new(null_mut()),
            free_head: UnsafeCell::new(null_mut()),
            grow_heap_fn: kernel_memory_map::grow_kernel_heap,
        }
    }

    pub unsafe fn init(&self, heap_start: usize, heap_size: usize) {
        let this = heap_start as *mut BlockHeader;
        *this = BlockHeader::new(heap_size - BLOCK_HEADER_SIZE, false);

        *self.head.get() = this;
        *self.tail.get() = this;
        *self.heap_end.get() = (heap_start + heap_size) as *mut BlockHeader;
        *self.free_head.get() = this;
    }

    unsafe fn insert_free_block(&self, this: *mut BlockHeader) {
        (*this).set_free_next(*self.free_head.get());
        (*this).set_free_prev(null_mut());

        if !(*self.free_head.get()).is_null() {
            (**self.free_head.get()).set_free_prev(this);
        }

        *self.free_head.get() = this;
    }

    unsafe fn remove_free_block(&self, this: *mut BlockHeader) {
        if !(*this).free_prev().is_null() {
            (*(*this).free_prev()).set_free_next((*this).free_next());
        } else {
            *self.free_head.get() = (*this).free_next();
        }

        if !(*this).free_next().is_null() {
            (*(*this).free_next()).set_free_prev((*this).free_prev());
        }

        (*this).set_free_next(null_mut());
        (*this).set_free_prev(null_mut());
    }

    unsafe fn append_to_list(&self, this: *mut BlockHeader) {
        let last = *self.tail.get();
        (*this).set_prev(last);

        *self.tail.get() = this;
    }

    unsafe fn insert_after(&self, this: *mut BlockHeader, new_block: *mut BlockHeader) {
        let next = (*this).next();

        if this != *self.tail.get() {
            (*next).set_prev(new_block);
        } else {
            *self.tail.get() = new_block;
        }

        (*new_block).set_prev(this);
    }

    unsafe fn remove_from_list(&self, this: *mut BlockHeader) {
        let next = (*this).next();
        let prev = (*this).prev();
        assert!(!prev.is_null(), "Head will never be removed");

        if this != *self.tail.get() {
            (*next).set_prev(prev);
        } else {
            *self.tail.get() = prev;
        }
    }

    unsafe fn find_fit(&self, layout: Layout) -> Option<(*mut BlockHeader, *mut BlockHeader)> {
        let total_needed = align_up(layout.size(), 8);
        let size = layout.size();
        let align = layout.align();

        let mut current_free = *self.free_head.get();

        if align <= 8 {
            while !current_free.is_null() {
                if (*current_free).size() >= total_needed {
                    return Some((current_free, current_free));
                }

                current_free = (*current_free).free_next();
            }
        } else {
            while !current_free.is_null() {
                if let Some(aligned_location) = check_aligned_fit(current_free, size, align) {
                    return Some((current_free, aligned_location));
                }

                current_free = (*current_free).free_next();
            }
        }

        let new_heap_size = (BLOCK_HEADER_SIZE + align + size).max(memory::PAGE_SIZE);
        let (new_heap, actual_size) = (self.grow_heap_fn)(new_heap_size)?;

        *self.heap_end.get() = (new_heap + actual_size) as *mut BlockHeader;

        let last = *self.tail.get();

        if (*last).is_free() {
            (*last).add_size(actual_size);
            let aligned_location =
                check_aligned_fit(last, size, align).expect("Failed to find aligned fit");
            return Some((last, aligned_location));
        } else {
            let this = new_heap as *mut BlockHeader;
            *this = BlockHeader::new(actual_size - BLOCK_HEADER_SIZE, false);

            self.insert_free_block(this);

            self.append_to_list(this);

            let aligned_location =
                check_aligned_fit(this, size, align).expect("Failed to find aligned fit");
            return Some((this, aligned_location));
        }
    }

    unsafe fn split_block(
        &self,
        this: *mut BlockHeader,
        aligned_block: *mut BlockHeader,
        layout: Layout,
    ) -> *mut u8 {
        if this == aligned_block {
            let total_needed = align_up(layout.size().max(MIN_ALLOC_SIZE), 8);
            let excess = (*this).size() - total_needed;
            if excess >= MIN_BLOCK_SIZE {
                let new_block = (*this).alloc_area_start().add(total_needed) as *mut BlockHeader;

                *new_block = BlockHeader::new(excess - BLOCK_HEADER_SIZE, false);

                self.insert_free_block(new_block);

                self.insert_after(this, new_block);

                (*this).set_size(total_needed);
            }

            self.remove_free_block(this);

            (*this).set_used();
            (*this).alloc_area_start()
        } else {
            //split into three blocks
            let this_size = (*this).size();
            let preceding_size = aligned_block as usize - (*this).alloc_area_start() as usize;
            let aligned_block_size = layout.size().max(MIN_BLOCK_SIZE as usize - BLOCK_HEADER_SIZE);
            let excess = this_size
                - (preceding_size + BLOCK_HEADER_SIZE)
                - (aligned_block_size + BLOCK_HEADER_SIZE);

            (*this).set_size(preceding_size);

            *aligned_block = BlockHeader::new(aligned_block_size, true);
            self.insert_after(this, aligned_block);

            if excess >= MIN_ALLOC_SIZE {
                let new_block_addr = (*aligned_block).end_ptr() as *mut BlockHeader;
                *new_block_addr = BlockHeader::new(excess, false);

                self.insert_free_block(new_block_addr);

                self.insert_after(aligned_block, new_block_addr);
            }

            (*aligned_block).alloc_area_start()
        }
    }

    unsafe fn dealloc_and_coalesce(&self, ptr: *mut BlockHeader) {
        let block_ptr = (ptr as usize - BLOCK_HEADER_SIZE) as *mut BlockHeader;
        (*block_ptr).set_free();

        let next = (*block_ptr).next();

        if block_ptr != *self.tail.get() && (*next).is_free() {
            (*block_ptr).add_size(BLOCK_HEADER_SIZE + (*next).size());

            self.remove_free_block(next);

            self.remove_from_list(next);
        }

        let prev = (*block_ptr).prev();

        if !prev.is_null() && (*prev).is_free() {
            self.remove_from_list(block_ptr);

            (*prev).add_size(BLOCK_HEADER_SIZE + (*block_ptr).size());
        } else {
            self.insert_free_block(block_ptr);
        }
    }

    #[allow(dead_code)]
    pub unsafe fn dump_heap(&self) {
        print!("\n--- Heap Dump Start ---");
        println!(" Head: {:?}, Free head: {:?}", *self.head.get(), *self.free_head.get(),);

        let mut current = *self.head.get();
        let mut index = 0;

        loop {
            println!(
                "Block {} at {:p}: size = {}, end = {:#x}, used = {}",
                index,
                current,
                (*current).size(),
                (*current).alloc_area_start() as usize + (*current).size(),
                (*current).is_used(),
            );
            println!("  Next: {:?}, Prev: {:?}", (*current).next(), (*current).prev(),);
            println!(
                "  Free Next: {:?}, Free Prev: {:?}",
                (*current).free_next(),
                (*current).free_prev(),
            );

            current = (*current).next();
            index += 1;

            if current == *self.heap_end.get() {
                break;
            }
        }

        print!("--- Heap Dump End ---");
        println!(" Tail: {:?}, Heap end: {:?}", *self.tail.get(), *self.heap_end.get());
    }
}

unsafe impl Sync for LinkedListAllocator {}

unsafe fn check_aligned_fit(
    block: *mut BlockHeader,
    alloc_size: usize,
    align: usize,
) -> Option<*mut BlockHeader> {
    let start = (*block).alloc_area_start();
    let end = (*block).end_ptr() as usize;

    let mut aligned = align_up(start as usize, align);

    while align_up(aligned + alloc_size, 8) <= end {
        let header = aligned - BLOCK_HEADER_SIZE;
        let padding = header - block as usize;

        if padding == 0 || padding >= MIN_BLOCK_SIZE {
            return Some(header as *mut BlockHeader);
        }

        aligned += align;
    }

    None
}
