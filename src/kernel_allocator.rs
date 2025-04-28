use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
use core::mem::size_of;
use core::cell::UnsafeCell;

use crate::kernel_memory_map;
use crate::memory;

const MIN_BLOCK_SIZE: usize = size_of::<BlockHeader>() + 8;

#[global_allocator]
pub static ALLOCATOR: LinkedListAllocator = LinkedListAllocator::new();

#[repr(C)]
#[derive(Debug)]
struct BlockHeader {
    size: usize,
    used: bool,
    next: Option<&'static mut BlockHeader>,
    prev: Option<&'static mut BlockHeader>,
    free_next: Option<&'static mut BlockHeader>,
    free_prev: Option<&'static mut BlockHeader>,
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
    head: UnsafeCell<Option<&'static mut BlockHeader>>,
    tail: UnsafeCell<Option<&'static mut BlockHeader>>,
    free_head: UnsafeCell<Option<&'static mut BlockHeader>>,
    grow_heap_fn: fn(usize) -> Option<(usize, usize)>,
}

impl LinkedListAllocator {
    pub const fn new() -> Self {
        LinkedListAllocator {
            head: UnsafeCell::new(None),
            tail: UnsafeCell::new(None),
            free_head: UnsafeCell::new(None),
            grow_heap_fn: kernel_memory_map::grow_kernel_heap,
        }
    }

    pub unsafe fn init(&self, heap_start: usize, heap_size: usize) {
        let block = heap_start as *mut BlockHeader;
        *block = BlockHeader {
            size: heap_size - size_of::<BlockHeader>(),
            used: false,
            next: None,
            prev: None,
            free_next: None,
            free_prev: None,
        };
        *self.head.get() = Some(&mut *block);
        *self.tail.get() = Some(&mut *block);
        *self.free_head.get() = Some(&mut *block);
    }

    unsafe fn find_fit(&self, layout: Layout) -> Option<&'static mut BlockHeader> {
        let mut current = match (*self.head.get()).as_mut() {
            Some(block) => *block as *mut BlockHeader,
            None => null_mut(),
        };

        while !current.is_null() {
            let curr = &mut *current;

            if !curr.used && curr.size >= layout.size() {
                return Some(curr);
            }

            current = match curr.next.as_mut() {
                Some(next_block) => *next_block as *mut BlockHeader,
                None => null_mut(),
            };
        }

        let size = (layout.size() + size_of::<BlockHeader>()).max(memory::PAGE_SIZE);
        let (new_addr, actual_size) = (self.grow_heap_fn)(size)?;
        let new_block = new_addr as *mut BlockHeader;

        let last_block = (*self.tail.get())
            .as_mut()
            .expect("Kernel allocator tail must exist");

        if !last_block.used {
            last_block.size += actual_size;
            return Some(last_block);
        } else {
            *new_block = BlockHeader {
                size: actual_size - size_of::<BlockHeader>(),
                used: false,
                next: None,
                prev: Some(unsafe { core::mem::transmute::<&mut BlockHeader, &'static mut BlockHeader>(last_block) }),
                free_next: None,
                free_prev: None,
            };
            last_block.next = Some(&mut *new_block);
            *self.tail.get() = Some(&mut *new_block);
            return Some(&mut *new_block);
        }
    }

    unsafe fn split_block(&self, block: &mut BlockHeader, layout: Layout) -> *mut u8 {
        let total_needed = layout.size().max(MIN_BLOCK_SIZE);
        let excess = block.size - total_needed;

        if excess > MIN_BLOCK_SIZE {
            let new_block_ptr = block.start_ptr().add(total_needed) as *mut BlockHeader;

            *(&mut *new_block_ptr) = BlockHeader {
                size: excess - size_of::<BlockHeader>(),
                used: false,
                next: block.next.take(),
                prev: Some(&mut *(block as *mut BlockHeader)),
                free_next: block.free_next.take(),
                free_prev: block.free_prev.take(),
            };

            let old_free_next_ptr = (*new_block_ptr).free_next.as_mut().map(|b| *b as *mut BlockHeader);
            let old_free_prev_ptr = (*new_block_ptr).free_prev.as_mut().map(|b| *b as *mut BlockHeader);

            if let Some(next_free_ptr) = old_free_next_ptr {
                (*next_free_ptr).free_prev = Some(&mut *new_block_ptr);
            }

            if let Some(prev_free_ptr) = old_free_prev_ptr {
                (*prev_free_ptr).free_next = Some(&mut *new_block_ptr);
            } else {
                *self.free_head.get() = Some(&mut *new_block_ptr);
            }

            if let Some(next_block) = (&mut *new_block_ptr).next.as_mut() {
                next_block.prev = Some(&mut *new_block_ptr);
            } else {
                *self.tail.get() = Some(&mut *new_block_ptr);
            }

            block.size = total_needed;
            block.next = Some(&mut *new_block_ptr);
        } else {
            if let Some(prev) = block.free_prev.take() {
                prev.free_next = block.free_next.take();
            } else {
                *self.free_head.get() = block.free_next.take();
            }

            if let Some(next) = block.free_next.as_mut() {
                next.free_prev = block.free_prev.take();
            }
        }

        block.used = true;
        block.start_ptr()
    }

    unsafe fn coalesce(&self) {
        let mut current = match (*self.head.get()).as_mut() {
            Some(block) => *block as *mut BlockHeader,
            None => null_mut(),
        };

        while !current.is_null() {
            let curr = &mut *current;
            let mut curr_end = curr.end_ptr();
            let curr_used = curr.used;

            while let Some(next) = curr.next.as_mut() {
                if !curr_used && !next.used && curr_end == next as *const _ as usize {
                    curr.size += size_of::<BlockHeader>() + next.size;
                    curr.next = next.next.take();
                    curr_end = curr.end_ptr();
                } else {
                    break;
                }
            }

            current = match curr.next.as_mut() {
                Some(next_block) => *next_block as *mut BlockHeader,
                None => null_mut(),
            };
        }
    }

    #[allow(dead_code)]
    pub unsafe fn dump_heap(&self) {
        print!("\n--- Heap Dump Start ---");
        println!(" Head: {:?}, Free head: {:?}",
                 (*self.head.get()).as_ref().map(|b| *b as *const _),
                 (*self.free_head.get()).as_ref().map(|b| *b as *const _)
         );

        let mut current = (*self.head.get()).as_ref();

        let mut index = 0;
        while let Some(block) = current {
            println!(
                "Block {} at {:p}: size = {}, end = {:#x}, used = {}",
                index,
                *block,
                block.size,
                (block.start_ptr() as usize) + block.size,
                block.used,
            );
            println!(
                "  Next: {:?}, Prev: {:?}, Free Next: {:?}, Free Prev: {:?}",
                block.next.as_ref().map(|b| *b as *const _),
                block.prev.as_ref().map(|b| *b as *const _),
                block.free_next.as_ref().map(|b| *b as *const _),
                block.free_prev.as_ref().map(|b| *b as *const _)
            );

            current = block.next.as_ref();
            index += 1;
        }

        print!("--- Heap Dump End ---");
        println!(" Tail: {:?}", (*self.tail.get()).as_ref().map(|b| *b as *const _));
    }
}

unsafe impl Sync for LinkedListAllocator {}

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

        let header_ptr = (ptr as usize - size_of::<BlockHeader>()) as *mut BlockHeader;
        (*header_ptr).used = false;

        //self.coalesce();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::alloc::Layout;

    const TEST_HEAP_SIZE: usize = memory::PAGE_SIZE * 2;

    #[repr(align(4096))]
    struct AlignedHeap([u8; TEST_HEAP_SIZE]);

    static mut TEST_HEAP: AlignedHeap = AlignedHeap([0; TEST_HEAP_SIZE]);

    static TEST_ALLOCATOR: LinkedListAllocator = LinkedListAllocator {
        head: UnsafeCell::new(None),
        tail: UnsafeCell::new(None),
        free_head: UnsafeCell::new(None),
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
            let existing_block_ptr = *header.next.as_ref().expect("Next block should exist") as *const BlockHeader;

            let alloc_size = 64;
            let layout = Layout::from_size_align(alloc_size, 8).unwrap();
            let ptr = TEST_ALLOCATOR.alloc(layout);

            assert!(!ptr.is_null(), "Allocation returned null pointer");

            assert!(header.used, "Allocated block should be marked as used");
            assert!(header.size >= alloc_size, "Allocated block too small");

            let next_block = header.next.as_ref().expect("Next block should exist after split");
            assert_ne!(*next_block as *const BlockHeader, existing_block_ptr, "Next block should not be the same as the existing block");

            assert!(!next_block.used, "Next block should be free after split");

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
            let existing_block_ptr = *header.next.as_ref().expect("Next block should exist") as *const BlockHeader;

            let layout = Layout::from_size_align(size, 8).unwrap();
            let ptr = TEST_ALLOCATOR.alloc(layout);

            assert!(!ptr.is_null(), "Exact fit allocation returned null pointer");

            assert!(header.used, "Exact fit block should be marked as used");
            assert!(header.size >= size, "Allocated block size incorrect for exact fit");

            let next_block_ptr = *header.next.as_ref().expect("Next block should exist after split") as *const BlockHeader;
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

            let initial_tail_addr = *initial_tail as *const _ as usize;
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

            let initial_tail = (*TEST_ALLOCATOR.tail.get())
                .as_ref()
                .expect("Tail must exist at start");

            let full_layout = Layout::from_size_align(initial_tail.size, 8).unwrap();
            let ptr = TEST_ALLOCATOR.alloc(full_layout);

            // Now the last block is fully used.

            // Step 2: Trigger growth by allocating more
            let layout = Layout::from_size_align(256, 8).unwrap();
            let new_ptr = TEST_ALLOCATOR.alloc(layout);
            assert!(!new_ptr.is_null(), "Allocation after growth failed");

            // Step 3: Tail should now point to a brand new block
            let new_tail = (*TEST_ALLOCATOR.tail.get())
                .as_ref()
                .expect("Tail must exist after growth");

            assert_ne!(
                *new_tail as *const _ as usize,
                initial_tail as *const _ as usize,
                "Tail should have moved (new block should have been created)"
            );

            assert!(!new_tail.used, "New tail block should be free (after growth)");

            assert_heap_invariants();
        }
    }

    #[test_case]
    fn test_free_list_after_init() {
        unsafe {
            println!("Testing free list after init...");
            setup_allocator();

            let free_head = (*TEST_ALLOCATOR.free_head.get())
                .as_ref()
                .expect("Free head must exist after init");

            let head = (*TEST_ALLOCATOR.head.get())
                .as_ref()
                .expect("Head block must exist after init");

            assert_eq!(
                *free_head as *const BlockHeader,
                *head as *const BlockHeader,
                "Free head should point to initial free block"
            );

            assert!(free_head.free_next.is_none(), "Free head should have no next free block");
            assert!(free_head.free_prev.is_none(), "Free head should have no prev free block");

            assert_heap_invariants();
        }
    }

    #[test_case]
    fn test_free_list_after_alloc_removes_block() {
        unsafe {
            println!("Testing free list after allocation removes block...");
            setup_allocator();

            let original_free_head = (*TEST_ALLOCATOR.free_head.get())
                .as_ref()
                .expect("Free head must exist after init");

            let original_free_head_ptr = *original_free_head as *const BlockHeader;

            let alloc_layout = Layout::from_size_align(128, 8).unwrap();
            let alloc_ptr = TEST_ALLOCATOR.alloc(alloc_layout);

            assert!(!alloc_ptr.is_null(), "Allocation failed unexpectedly");

            let mut free_current = (*TEST_ALLOCATOR.free_head.get()).as_ref();

            while let Some(block) = free_current {
                let block_addr = *block as *const BlockHeader;
                assert_ne!(
                    block_addr,
                    original_free_head_ptr,
                    "Allocated block still present in free list after allocation"
                );
                free_current = block.free_next.as_ref();
            }

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
        let mut current = (*TEST_ALLOCATOR.head.get()).as_ref();
        let mut last_block_addr = 0usize;

        while let Some(block) = current {
            let block_addr = *block as *const _ as usize;

            assert_eq!(block_addr % 8, 0, "Block address not properly aligned: {:p}", *block);

            assert!(block.size > 0, "Block size must be greater than 0");

            assert!(block_addr > last_block_addr, "Block addresses not strictly increasing");

            if let Some(next_block) = block.next.as_ref() {
                let next_prev_ptr = next_block.prev.as_ref()
                    .expect("Next block should have a prev pointer");
                assert_eq!(
                    *next_prev_ptr as *const _,
                    *block as *const _,
                    "Next block's prev does not point back to current block"
                );
            }

            if let Some(prev_block) = block.prev.as_ref() {
                let prev_next_ptr = prev_block.next.as_ref()
                    .expect("Prev block should have a next pointer");
                assert_eq!(
                    *prev_next_ptr as *const _,
                    *block as *const _,
                    "Prev block's next does not point forward to current block"
                );
            }

            last_block_addr = block_addr;

            current = block.next.as_ref();
        }

        let tail_block = (*TEST_ALLOCATOR.tail.get())
            .as_ref()
            .expect("Allocator tail must exist");
        let tail_block_addr = *tail_block as *const _ as usize;

        assert_eq!(
            last_block_addr,
            tail_block_addr,
            "Allocator tail does not point to last block in heap"
        );

        let mut free_current = (*TEST_ALLOCATOR.free_head.get()).as_ref();
        let mut last_free_addr = 0usize;

        while let Some(free_block) = free_current {
            let block_addr = *free_block as *const _ as usize;

            assert!(
                !free_block.used,
                "Free block is marked used: {:p}",
                *free_block
            );

            if let Some(next_free_block) = free_block.free_next.as_ref() {
                let next_free_prev = next_free_block.free_prev.as_ref()
                    .expect("Next free block should have a free_prev pointer");
                assert_eq!(
                    *next_free_prev as *const _,
                    *free_block as *const _,
                    "Next free block's free_prev does not point back to current free block"
                );
            }

            if let Some(prev_free_block) = free_block.free_prev.as_ref() {
                let prev_free_next = prev_free_block.free_next.as_ref()
                    .expect("Prev free block should have a free_next pointer");
                assert_eq!(
                    *prev_free_next as *const _,
                    *free_block as *const _,
                    "Prev free block's free_next does not point forward to current free block"
                );
            }

            last_free_addr = block_addr;
            free_current = free_block.free_next.as_ref();
        }
    }
}
