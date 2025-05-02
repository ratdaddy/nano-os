use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
use core::mem::size_of;
use core::cell::UnsafeCell;

use crate::kernel_memory_map;
use crate::memory;

const MIN_BLOCK_SIZE: usize = size_of::<BlockHeader>() + 8;

#[global_allocator]
pub static ALLOCATOR: LinkedListAllocator = LinkedListAllocator::new();

unsafe impl GlobalAlloc for LinkedListAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if let Some(block) = self.find_fit(layout) {
            self.split_block(block, layout)
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

#[repr(C)]
#[derive(Debug)]
struct BlockHeader {
    size: usize,
    used: bool,
    next: *mut BlockHeader,
    prev: *mut BlockHeader,
    free_next: *mut BlockHeader,
    free_prev: *mut BlockHeader,
}

impl BlockHeader {
    fn start_ptr(&self) -> *mut u8 {
        unsafe {
            (self as *const _ as *mut u8).add(size_of::<BlockHeader>())
        }
    }

    fn end_ptr(&self) -> usize {
        self as *const _ as usize + size_of::<BlockHeader>() + self.size
    }
}

pub struct LinkedListAllocator {
    head: UnsafeCell<*mut BlockHeader>,
    tail: UnsafeCell<*mut BlockHeader>,
    free_head: UnsafeCell<*mut BlockHeader>,
    grow_heap_fn: fn(usize) -> Option<(usize, usize)>,
}

impl LinkedListAllocator {
    pub const fn new() -> Self {
        LinkedListAllocator {
            head: UnsafeCell::new(null_mut()),
            tail: UnsafeCell::new(null_mut()),
            free_head: UnsafeCell::new(null_mut()),
            grow_heap_fn: kernel_memory_map::grow_kernel_heap,
        }
    }

    pub unsafe fn init(&self, heap_start: usize, heap_size: usize) {
        let this = heap_start as *mut BlockHeader;
        *this = BlockHeader {
            size: heap_size - size_of::<BlockHeader>(),
            used: false,
            next: null_mut(),
            prev: null_mut(),
            free_next: null_mut(),
            free_prev: null_mut(),
        };
        *self.head.get() = this;
        *self.tail.get() = this;
        *self.free_head.get() = this;
    }

    unsafe fn insert_free_block(&self, this: *mut BlockHeader) {
        (*this).free_next = *self.free_head.get();
        (*this).free_prev = null_mut();

        if !(*self.free_head.get()).is_null() {
            (**self.free_head.get()).free_prev = this;
        }

        *self.free_head.get() = this;
    }

    unsafe fn remove_free_block(&self, this: *mut BlockHeader) {
        if !(*this).free_prev.is_null() {
            (*(*this).free_prev).free_next = (*this).free_next;
        } else {
            *self.free_head.get() = (*this).free_next;
        }

        if !(*this).free_next.is_null() {
            (*(*this).free_next).free_prev = (*this).free_prev;
        }
    }

    unsafe fn append_to_list(&self, this: *mut BlockHeader) {
        let last = *self.tail.get();

        (*this).next = null_mut();
        (*this).prev = last;

        (*last).next = this;

        *self.tail.get() = this;
    }

    unsafe fn insert_after(&self, this: *mut BlockHeader, new_block: *mut BlockHeader) {
            let next = (*this).next;

            if !next.is_null() {
                (*next).prev = new_block;
            } else {
                *self.tail.get() = new_block;
            }
            (*new_block).next = next;

            (*this).next = new_block;
            (*new_block).prev = this;
    }

    unsafe fn remove_from_list(&self, this: *mut BlockHeader) {
        let next = (*this).next;
        let prev = (*this).prev;
        assert!(!prev.is_null(), "Head will never be removed");

        if !next.is_null() {
            (*next).prev = prev;
            (*prev).next = next;
        } else {
            println!("***************** THIS NEEDS A TEST: B *****************");
            *self.tail.get() = prev;

        }

        (*prev).next = next;
    }

    unsafe fn find_fit(&self, layout: Layout) -> Option<*mut BlockHeader> {
        let mut current_free = *self.free_head.get();

        while !current_free.is_null() {
            if !(*current_free).used && (*current_free).size >= layout.size() {
                return Some(current_free);
            }

            current_free = (*current_free).free_next;
        }

        let size = (layout.size() + size_of::<BlockHeader>()).max(memory::PAGE_SIZE);
        let (new_heap, actual_size) = (self.grow_heap_fn)(size)?;

        let last = *self.tail.get();
        assert!(!last.is_null());

        if !(*last).used {
            (*last).size += actual_size;
            return Some(last);
        } else {
            let this = new_heap as *mut BlockHeader;
            *this = BlockHeader {
                size: actual_size - size_of::<BlockHeader>(),
                used: false,
                next: null_mut(),
                prev: null_mut(),
                free_next: null_mut(),
                free_prev: null_mut(),
            };

            self.insert_free_block(this);

            self.append_to_list(this);

            return Some(this);
        }
    }

    unsafe fn split_block(&self, this: *mut BlockHeader, layout: Layout) -> *mut u8 {
        let total_needed = layout.size().max(MIN_BLOCK_SIZE);
        let excess = (*this).size - total_needed;

        if excess > MIN_BLOCK_SIZE {
            let new_block = (*this).start_ptr().add(total_needed) as *mut BlockHeader;

            *new_block = BlockHeader {
                size: excess - size_of::<BlockHeader>(),
                used: false,
                next: null_mut(),
                prev: null_mut(),
                free_next: null_mut(),
                free_prev: null_mut(),
            };

            self.insert_free_block(new_block);

            self.insert_after(this, new_block);

            (*this).size = total_needed;
        }

        self.remove_free_block(this);

        (*this).used = true;
        (*this).start_ptr()
    }

    unsafe fn dealloc_and_coalesce(&self, ptr: *mut BlockHeader) {
        let block_ptr = (ptr as usize - size_of::<BlockHeader>()) as *mut BlockHeader;
        (*block_ptr).used = false;

        let next = (*block_ptr).next;

        if !next.is_null() && !(*next).used {
            (*block_ptr).size += size_of::<BlockHeader>() + (*next).size;

            self.remove_free_block(next);

            self.remove_from_list(next);
        }

        let prev = (*block_ptr).prev;

        if !prev.is_null() && !(*prev).used {
            self.remove_from_list(block_ptr);

            (*prev).size += size_of::<BlockHeader>() + (*block_ptr).size;
        } else {
            self.insert_free_block(block_ptr);
        }
    }

    #[allow(dead_code)]
    pub unsafe fn dump_heap(&self) {
        print!("\n--- Heap Dump Start ---");
        println!(" Head: {:?}, Free head: {:?}",
            *self.head.get(),
            *self.free_head.get(),
        );

        let mut current = *self.head.get();
        let mut index = 0;

        while !current.is_null() {
            println!(
                "Block {} at {:p}: size = {}, end = {:#x}, used = {}",
                index,
                current,
                (*current).size,
                ((*current).start_ptr() as usize) + (*current).size,
                (*current).used,
            );
            println!(
                "  Next: {:?}, Prev: {:?}",
                (*current).next,
                (*current).prev,
            );
            println!("  Free Next: {:?}, Free Prev: {:?}",
                (*current).free_next,
                (*current).free_prev,
            );

            current = (*current).next;
            index += 1;
        }

        print!("--- Heap Dump End ---");
        println!(" Tail: {:?}", *self.tail.get());
    }
}

unsafe impl Sync for LinkedListAllocator {}

#[cfg(test)]
mod tests {
    use super::*;
    use core::alloc::Layout;

    const TEST_HEAP_SIZE: usize = memory::PAGE_SIZE * 2;

    #[repr(align(4096))]
    struct AlignedHeap([u8; TEST_HEAP_SIZE]);

    static mut TEST_HEAP: AlignedHeap = AlignedHeap([0; TEST_HEAP_SIZE]);

    static TEST_ALLOCATOR: LinkedListAllocator = LinkedListAllocator {
        head: UnsafeCell::new(null_mut()),
        tail: UnsafeCell::new(null_mut()),
        free_head: UnsafeCell::new(null_mut()),
        grow_heap_fn: test_grow_heap,
    };

    #[test_case]
    fn test_basic_allocator() {
        unsafe {
            println!("Testing basic allocator...");
            setup_allocator();

            let alloc_size = 32;
            let layout = Layout::from_size_align(alloc_size, 8).unwrap();
            let ptr = TEST_ALLOCATOR.alloc(layout);
            assert!(!ptr.is_null());

            let header_ptr = (ptr as usize - size_of::<BlockHeader>()) as *const BlockHeader;
            let header = &*header_ptr;
            assert!(header.used, "Block should be marked as used");
            assert!(header.size >= alloc_size, "Block size should be at least 32 bytes");
        }
    }

    #[test_case]
    fn test_split_allocation() {
        unsafe {
            println!("Testing split allocation...");
            let (freed_block, _size) = setup_fragmented_heap_for_test();

            let header_ptr = (freed_block as usize - core::mem::size_of::<BlockHeader>()) as *const BlockHeader;
            let header = &*header_ptr;
            let existing_block_ptr = header.next;

            let alloc_size = 64;
            let layout = Layout::from_size_align(alloc_size, 8).unwrap();
            let ptr = TEST_ALLOCATOR.alloc(layout);

            assert!(!ptr.is_null(), "Allocation returned null pointer");

            assert!(header.used, "Allocated block should be marked as used");
            assert!(header.size >= alloc_size, "Allocated block too small");

            let next_block = header.next;
            assert_ne!(next_block, existing_block_ptr, "Next block should not be the same as the existing block");

            assert!(!(*next_block).used, "Next block should be free after split");

            assert_heap_invariants();
        }
    }

    #[test_case]
    fn test_exact_fit_allocation() {
        unsafe {
            println!("Testing exact fit allocation...");
            let (freed_block, size) = setup_fragmented_heap_for_test();

            let header_ptr = (freed_block as usize - core::mem::size_of::<BlockHeader>()) as *const BlockHeader;
            let header = &*header_ptr;
            let existing_block_ptr = (*header).next;

            let layout = Layout::from_size_align(size, 8).unwrap();
            let ptr = TEST_ALLOCATOR.alloc(layout);

            assert!(!ptr.is_null(), "Exact fit allocation returned null pointer");

            assert!(header.used, "Exact fit block should be marked as used");
            assert!(header.size >= size, "Allocated block size incorrect for exact fit");

            let next_block_ptr = (*header).next;
            assert_eq!(existing_block_ptr, next_block_ptr,
                "Exact fit block should have the same next as previously (no split should have occurred)"
            );

            assert_heap_invariants();
        }
    }

    #[test_case]
    fn test_heap_growth_extend_free_block() {
        unsafe {
            println!("Testing heap growth with existing free block...");
            setup_allocator();

            let _initial_ptr = TEST_ALLOCATOR.alloc(Layout::from_size_align(3072, 8).unwrap());

            let initial_tail = (*TEST_ALLOCATOR.tail.get())
                .as_ref()
                .expect("Tail must exist at start");

            let initial_tail_addr = initial_tail as *const _ as usize;
            let initial_tail_size = initial_tail.size;

            let alloc_size = 1024;
            let layout = Layout::from_size_align(alloc_size, 8).unwrap();
            let ptr = TEST_ALLOCATOR.alloc(layout);

            assert!(!ptr.is_null(), "Allocation after growth failed");

            let new_tail = (*TEST_ALLOCATOR.tail.get())
                .as_ref()
                .expect("Tail must exist after growth");

            assert!(initial_tail.size >= alloc_size, "Tail block size should have grown");

            assert_heap_invariants();
        }
    }

    #[test_case]
    fn test_heap_growth_create_new_block() {
        unsafe {
            println!("Testing heap growth with new block creation...");
            setup_allocator();

            let initial_tail = *TEST_ALLOCATOR.tail.get();

            let full_layout = Layout::from_size_align((*initial_tail).size, 8).unwrap();
            let ptr = TEST_ALLOCATOR.alloc(full_layout);

            let layout = Layout::from_size_align(256, 8).unwrap();
            let new_ptr = TEST_ALLOCATOR.alloc(layout);
            assert!(!new_ptr.is_null(), "Allocation after growth failed");

            let new_tail = *TEST_ALLOCATOR.tail.get();

            assert_ne!(
                new_tail,
                initial_tail,
                "Tail should have moved (new block should have been created)"
            );

            assert!(!(*new_tail).used, "New tail block should be free (after growth)");

            assert_heap_invariants();
        }
    }

    #[test_case]
    fn test_free_list_after_init() {
        unsafe {
            println!("Testing free list after init...");
            setup_allocator();

            let free_head = *TEST_ALLOCATOR.free_head.get();

            let head = *TEST_ALLOCATOR.head.get();

            assert_eq!(
                free_head,
                head,
                "Free head should point to initial free block"
            );

            assert!((*free_head).free_next.is_null(), "Free head should have no next free block");
            assert!((*free_head).free_prev.is_null(), "Free head should have no prev free block");

            assert_heap_invariants();
        }
    }

    #[test_case]
    fn test_free_list_after_alloc_removes_block() {
        unsafe {
            println!("Testing free list after allocation removes block...");
            setup_allocator();

            let original_free_head = *TEST_ALLOCATOR.free_head.get();

            let alloc_layout = Layout::from_size_align(128, 8).unwrap();
            let alloc_ptr = TEST_ALLOCATOR.alloc(alloc_layout);

            assert!(!alloc_ptr.is_null(), "Allocation failed unexpectedly");

            let mut free_current = *TEST_ALLOCATOR.free_head.get();

            while !free_current.is_null() {
                assert_ne!(
                    free_current,
                    original_free_head,
                    "Allocated block still present in free list after allocation"
                );
                free_current = (*free_current).free_next;
            }

            assert_heap_invariants();
        }
    }

    #[test_case]
    fn test_coalesce_none() {
        unsafe {
            println!("Testing coalesce with no adjacent free blocks...");
            setup_allocator();

            let layout = Layout::from_size_align(128, 8).unwrap();
            let block1 = TEST_ALLOCATOR.alloc(layout);
            let block2 = TEST_ALLOCATOR.alloc(layout);
            let block3 = TEST_ALLOCATOR.alloc(layout);
TEST_ALLOCATOR.dealloc(block2, layout);

            // No coalescing should happen since block1 and block3 are both used
            let header_ptr = (block2 as usize - core::mem::size_of::<BlockHeader>()) as *mut BlockHeader;
            let header = &*header_ptr;

            assert_eq!(header.size, 128, "Freed block should retain its size after no coalescing");
            assert!(!header.used, "Freed block should be marked free");

            assert_heap_invariants();
        }
    }

    #[test_case]
    fn test_coalesce_with_next() {
        unsafe {
            println!("Testing coalesce with next free block...");
            setup_allocator();

            let layout = Layout::from_size_align(128, 8).unwrap();
            let block1 = TEST_ALLOCATOR.alloc(layout);
            let block2 = TEST_ALLOCATOR.alloc(layout);
            let block3 = TEST_ALLOCATOR.alloc(layout);
            let block4 = TEST_ALLOCATOR.alloc(layout);

            TEST_ALLOCATOR.dealloc(block3, layout);
            TEST_ALLOCATOR.dealloc(block2, layout);

            let header_ptr = (block2 as usize - core::mem::size_of::<BlockHeader>()) as *mut BlockHeader;
            let header = &*header_ptr;

            assert_eq!(
                header.size,
                128 + core::mem::size_of::<BlockHeader>() + 128,
                "Block2 should have absorbed Block3"
            );

            assert!(!header.used, "Merged block should be free");

            assert_heap_invariants();
        }
    }

    #[test_case]
    fn test_coalesce_with_prev() {
        unsafe {
            println!("Testing coalesce with previous free block...");
            setup_allocator();

            let layout = Layout::from_size_align(128, 8).unwrap();
            let block1 = TEST_ALLOCATOR.alloc(layout);
            let block2 = TEST_ALLOCATOR.alloc(layout);
            let block3 = TEST_ALLOCATOR.alloc(layout);

            TEST_ALLOCATOR.dealloc(block1, layout);
            TEST_ALLOCATOR.dealloc(block2, layout);

            let header_ptr = (block1 as usize - core::mem::size_of::<BlockHeader>()) as *mut BlockHeader;
            let header = &*header_ptr;

            // Should have merged block1 and block2
            assert_eq!(
                header.size,
                128 + core::mem::size_of::<BlockHeader>() + 128,
                "Block1 should have absorbed Block2"
            );

            assert!(!header.used, "Merged block should be free");

            assert_heap_invariants();
        }
    }

    #[test_case]
    fn test_coalesce_with_prev_and_next() {
        unsafe {
            println!("Testing coalesce with both previous and next free blocks...");
            setup_allocator();

            let layout = Layout::from_size_align(128, 8).unwrap();
            let block1 = TEST_ALLOCATOR.alloc(layout);
            let block2 = TEST_ALLOCATOR.alloc(layout);
            let block3 = TEST_ALLOCATOR.alloc(layout);
            let block4 = TEST_ALLOCATOR.alloc(layout);

            TEST_ALLOCATOR.dealloc(block3, layout);
            TEST_ALLOCATOR.dealloc(block2, layout);

            let header_ptr = (block2 as usize - core::mem::size_of::<BlockHeader>()) as *mut BlockHeader;
            let header = &*header_ptr;

            // Should have merged block2 and block3 into one block
            assert_eq!(
                header.size,
                128 + core::mem::size_of::<BlockHeader>() + 128,
                "Block2 and Block3 should have merged"
            );

            assert!(!header.used, "Merged block should be free");

            // Now free block1 and trigger merging of block1 + (block2+block3)
            TEST_ALLOCATOR.dealloc(block1, layout);

            let final_header_ptr = (block1 as usize - core::mem::size_of::<BlockHeader>()) as *mut BlockHeader;
            let final_header = &*final_header_ptr;

            let expected_total_size = 128
                + core::mem::size_of::<BlockHeader>()
                + 128
                + core::mem::size_of::<BlockHeader>()
                + 128;

            assert_eq!(
                final_header.size,
                expected_total_size,
                "Block1, Block2, and Block3 should have all merged"
            );

            assert!(!final_header.used, "Merged block should be free");

            assert_heap_invariants();
        }
    }

    unsafe fn setup_allocator() {
        TEST_ALLOCATOR.init(TEST_HEAP.0.as_ptr() as usize, TEST_HEAP_SIZE / 2);
    }

    fn test_grow_heap(size: usize) -> Option<(usize, usize)> {
        let heap_start = unsafe { TEST_HEAP.0.as_ptr() as usize };
        let second_half_start = heap_start + (TEST_HEAP_SIZE / 2);
        let second_half_size = TEST_HEAP_SIZE / 2;

        // Only allow a single grow
        if size > second_half_size {
            None // Requested too much
        } else {
            Some((second_half_start, second_half_size))
        }
    }

    unsafe fn setup_fragmented_heap_for_test() -> (*mut u8, usize) {
        setup_allocator();

        let small_layout = Layout::from_size_align(128, 8).unwrap();
        let small_ptr = TEST_ALLOCATOR.alloc(small_layout);

        let remaining_block = (*TEST_ALLOCATOR.head.get())
            .as_ref()
            .expect("Allocator head missing")
            .next
            .as_ref()
            .expect("Small block should have next block after it");

        let remaining_size = remaining_block.size;

        let big_layout = Layout::from_size_align(remaining_size, 8).unwrap();
        let big_ptr = TEST_ALLOCATOR.alloc(big_layout);

        TEST_ALLOCATOR.dealloc(small_ptr, small_layout);

        (small_ptr, 128) // Return the freed block's ptr and size for the test
    }

    unsafe fn assert_heap_invariants() {
        let mut current = *TEST_ALLOCATOR.head.get();
        let mut last_block_addr = null_mut();

        while !current.is_null() {
            assert_eq!(current as usize % 8, 0, "Block address not properly aligned: {:p}", current);

            assert!((*current).size > 0, "Block size must be greater than 0");

            assert!(current > last_block_addr, "Block addresses not strictly increasing");

            let next_ptr = (*current).next;
            if !next_ptr.is_null() {
                let next_prev_ptr = (*next_ptr).prev;
                assert_eq!(
                    next_prev_ptr,
                    current,
                    "Next block's prev does not point back to current block"
                );
            }

            let prev_ptr = (*current).prev;
            if !prev_ptr.is_null() {
                let prev_next_ptr = (*prev_ptr).next;
                assert_eq!(
                    prev_next_ptr,
                    current,
                    "Prev block's next does not point forward to current block"
                );
            }

            last_block_addr = current;

            current = (*current).next;
        }

        let tail_block = *TEST_ALLOCATOR.tail.get();

        assert_eq!(
            last_block_addr,
            tail_block,
            "Allocator tail does not point to last block in heap"
        );

        let mut free_current = *TEST_ALLOCATOR.free_head.get();
        let mut last_free_addr = null_mut();

        while !free_current.is_null() {
            let block_addr = free_current;

            assert!(
                !(*free_current).used,
                "Free block is marked used: {:p}",
                free_current
            );

            let next_free_block = (*free_current).free_next;
            if !next_free_block.is_null() {
                let next_free_prev = (*next_free_block).free_prev;
                assert_eq!(
                    next_free_prev,
                    free_current,
                    "Next free block's free_prev does not point back to current free block"
                );
            }

            let prev_free_block = (*free_current).free_prev;
            if !prev_free_block.is_null() {
                let prev_free_next = (*prev_free_block).free_next;
                assert_eq!(
                    prev_free_next,
                    free_current,
                    "Prev free block's free_next does not point forward to current free block"
                );
            }

            last_free_addr = block_addr;
            free_current = (*free_current).free_next;
        }

        let mut mem_current = *TEST_ALLOCATOR.head.get();

        while !mem_current.is_null() {
            if !(*mem_current).used {
                let mut found_in_free_list = false;

                let mut free_current = *TEST_ALLOCATOR.free_head.get();
                while !free_current.is_null() {
                    if free_current == mem_current {
                        found_in_free_list = true;
                        break;
                    }
                    free_current = (*free_current).free_next;
                }

                assert!(
                    found_in_free_list,
                    "Block {:p} marked free but not found in free list",
                    mem_current
                );
            }

            mem_current = (*mem_current).next;
        }
    }
}
