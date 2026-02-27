use core::alloc::GlobalAlloc;
use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::ptr::null_mut;

use super::allocator::LinkedListAllocator;
use super::allocator::{MIN_ALLOC_SIZE, MIN_BLOCK_SIZE};
use super::block_header::*;
use super::block_header::{TEST_HEAP, TEST_HEAP_SIZE};

static TEST_ALLOCATOR: LinkedListAllocator = LinkedListAllocator {
    head: UnsafeCell::new(null_mut()),
    tail: UnsafeCell::new(null_mut()),
    heap_end: UnsafeCell::new(null_mut()),
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

        let header = (ptr as usize - BLOCK_HEADER_SIZE) as *const BlockHeader;
        assert!((*header).is_used(), "Block should be marked as used");
        assert!((*header).size() >= alloc_size, "Block size should be at least 32 bytes");

        assert_heap_invariants();
    }
}

#[test_case]
fn test_split_allocation() {
    unsafe {
        println!("Testing split allocation...");
        let (freed, _size) = setup_fragmented_heap_for_test();

        let this = (freed as usize - BLOCK_HEADER_SIZE) as *const BlockHeader;
        let existing_block = (*this).next();

        let alloc_size = 64;
        let layout = Layout::from_size_align(alloc_size, 8).unwrap();
        let ptr = TEST_ALLOCATOR.alloc(layout);

        assert!(!ptr.is_null(), "Allocation returned null pointer");

        assert!((*this).is_used(), "Allocated block should be marked as used");
        assert!((*this).size() >= alloc_size, "Allocated block too small");

        let next = (*this).next();
        assert_ne!(next, existing_block, "Next block should not be the same as the existing block");

        assert!((*next).is_free(), "Next block should be free after split");

        assert_heap_invariants();
    }
}

#[test_case]
fn test_split_minimum_block_size() {
    unsafe {
        println!("Testing split with minimum block size...");
        setup_allocator();

        let this = *TEST_ALLOCATOR.head.get();

        let alloc_size = 8;
        let layout = Layout::from_size_align(alloc_size, 8).unwrap();
        let ptr = TEST_ALLOCATOR.alloc(layout);

        assert!(!ptr.is_null(), "Allocation returned null pointer");

        assert!((*this).is_used(), "Allocated block should be marked as used");
        assert!((*this).size() >= alloc_size, "Allocated block too small");
        assert!(
            (*this).size() + BLOCK_HEADER_SIZE == MIN_BLOCK_SIZE,
            "Allocated block should be minimum size"
        );

        assert_heap_invariants();
    }
}

#[test_case]
fn test_exact_fit_allocation() {
    unsafe {
        println!("Testing exact fit allocation...");
        let (freed, size) = setup_fragmented_heap_for_test();

        let this = (freed as usize - BLOCK_HEADER_SIZE) as *const BlockHeader;
        let existing_block = (*this).next();

        let layout = Layout::from_size_align(size as usize, 8).unwrap();
        let ptr = TEST_ALLOCATOR.alloc(layout);

        assert!(!ptr.is_null(), "Exact fit allocation returned null pointer");

        assert!((*this).is_used(), "Exact fit block should be marked as used");
        assert!((*this).size() >= size, "Allocated block size incorrect for exact fit");

        let next = (*this).next();
        assert_eq!(existing_block, next,
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

        let _ptr = TEST_ALLOCATOR.alloc(Layout::from_size_align(3072, 8).unwrap());

        let initial_tail = *TEST_ALLOCATOR.tail.get();

        let alloc_size = 1024;
        let layout = Layout::from_size_align(alloc_size, 8).unwrap();
        let ptr = TEST_ALLOCATOR.alloc(layout);

        assert!(!ptr.is_null(), "Allocation after growth failed");

        let new_tail = *TEST_ALLOCATOR.tail.get();

        assert!((*initial_tail).size() >= alloc_size, "Tail block size should have grown");
        assert!(new_tail != initial_tail, "Tail should have moved");

        assert_heap_invariants();
    }
}

#[test_case]
fn test_heap_growth_create_new_block() {
    unsafe {
        println!("Testing heap growth with new block creation...");
        setup_allocator();

        let initial_tail = *TEST_ALLOCATOR.tail.get();

        let full_layout = Layout::from_size_align((*initial_tail).size(), 8).unwrap();
        let _ptr = TEST_ALLOCATOR.alloc(full_layout);

        let layout = Layout::from_size_align(256, 8).unwrap();
        let new_ptr = TEST_ALLOCATOR.alloc(layout);
        assert!(!new_ptr.is_null(), "Allocation after growth failed");

        let new_tail = *TEST_ALLOCATOR.tail.get();

        assert_ne!(
            new_tail, initial_tail,
            "Tail should have moved (new block should have been created)"
        );

        assert!((*new_tail).is_free(), "New tail block should be free (after growth)");

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

        assert_eq!(free_head, head, "Free head should point to initial free block");

        assert!((*free_head).free_next().is_null(), "Free head should have no next free block");
        assert!((*free_head).free_prev().is_null(), "Free head should have no prev free block");

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
                free_current, original_free_head,
                "Allocated block still present in free list after allocation"
            );
            free_current = (*free_current).free_next();
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
        let _block1 = TEST_ALLOCATOR.alloc(layout);
        let block2 = TEST_ALLOCATOR.alloc(layout);
        let _block3 = TEST_ALLOCATOR.alloc(layout);

        TEST_ALLOCATOR.dealloc(block2, layout);

        let header2 = (block2 as usize - BLOCK_HEADER_SIZE) as *mut BlockHeader;

        assert_eq!(
            (*header2).size(),
            128,
            "Freed block should retain its size after no coalescing"
        );
        assert!((*header2).is_free(), "Freed block should be marked free");

        assert_heap_invariants();
    }
}

#[test_case]
fn test_coalesce_with_next() {
    unsafe {
        println!("Testing coalesce with next free block...");
        setup_allocator();

        let layout = Layout::from_size_align(128, 8).unwrap();
        let _block1 = TEST_ALLOCATOR.alloc(layout);
        let block2 = TEST_ALLOCATOR.alloc(layout);
        let block3 = TEST_ALLOCATOR.alloc(layout);
        let _block4 = TEST_ALLOCATOR.alloc(layout);

        TEST_ALLOCATOR.dealloc(block3, layout);
        TEST_ALLOCATOR.dealloc(block2, layout);

        let header2 = (block2 as usize - BLOCK_HEADER_SIZE) as *mut BlockHeader;

        assert_eq!(
            (*header2).size(),
            128 + BLOCK_HEADER_SIZE + 128,
            "Block2 should have absorbed Block3"
        );

        assert!((*header2).is_free(), "Merged block should be free");

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
        let _block3 = TEST_ALLOCATOR.alloc(layout);

        TEST_ALLOCATOR.dealloc(block1, layout);
        TEST_ALLOCATOR.dealloc(block2, layout);

        let header1 = (block1 as usize - BLOCK_HEADER_SIZE) as *mut BlockHeader;
        assert_eq!(
            (*header1).size(),
            128 + BLOCK_HEADER_SIZE + 128,
            "Block1 should have absorbed Block2"
        );

        assert!((*header1).is_free(), "Merged block should be free");

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
        let _block4 = TEST_ALLOCATOR.alloc(layout);

        TEST_ALLOCATOR.dealloc(block3, layout);
        TEST_ALLOCATOR.dealloc(block1, layout);
        TEST_ALLOCATOR.dealloc(block2, layout);

        let header1 = (block1 as usize - BLOCK_HEADER_SIZE) as *mut BlockHeader;

        let expected_total_size = 128 + BLOCK_HEADER_SIZE + 128 + BLOCK_HEADER_SIZE + 128;

        assert_eq!(
            (*header1).size(),
            expected_total_size,
            "Block1, Block2, and Block3 should have all merged"
        );

        assert!((*header1).is_free(), "Merged block should be free");

        assert_heap_invariants();
    }
}

#[test_case]
fn test_coalesce_all_blocks() {
    unsafe {
        println!("Testing coalesce with all blocks free...");
        setup_allocator();
        let original_size = (*(*TEST_ALLOCATOR.head.get())).size();

        let layout = Layout::from_size_align(128, 8).unwrap();
        let block1 = TEST_ALLOCATOR.alloc(layout);
        let block2 = TEST_ALLOCATOR.alloc(layout);
        let block3 = TEST_ALLOCATOR.alloc(layout);
        let block4 = TEST_ALLOCATOR.alloc(layout);

        TEST_ALLOCATOR.dealloc(block1, layout);
        TEST_ALLOCATOR.dealloc(block2, layout);
        TEST_ALLOCATOR.dealloc(block3, layout);
        TEST_ALLOCATOR.dealloc(block4, layout);

        let header1 = (block1 as usize - BLOCK_HEADER_SIZE) as *mut BlockHeader;

        assert_eq!((*header1).size(), original_size, "All blocks should have merged into one");

        assert!((*header1).is_free(), "Merged block should be free");

        assert_heap_invariants();
    }
}

#[test_case]
fn test_end_alignment() {
    unsafe {
        println!("Testing end alignment...");
        setup_allocator();

        let layout = Layout::from_size_align(101, 8).unwrap();
        let ptr = TEST_ALLOCATOR.alloc(layout);

        assert!(!ptr.is_null(), "Allocation failed unexpectedly");

        let header = (ptr as usize - BLOCK_HEADER_SIZE) as *const BlockHeader;

        let end = (*header).end_ptr() as usize;
        assert_eq!(end % 8, 0, "End address of allocated block should be aligned to 8 bytes");

        assert_heap_invariants();
    }
}

#[test_case]
fn test_basic_alignment() {
    unsafe {
        println!("Testing basic alignment...");
        setup_allocator();

        let layout = Layout::from_size_align(128, 128).unwrap();
        let ptr = TEST_ALLOCATOR.alloc(layout);

        assert!(!ptr.is_null(), "Allocation failed unexpectedly");

        assert_eq!(ptr as usize % 128, 0, "Allocated pointer should be aligned to 128 bytes");

        assert_heap_invariants();
    }
}

#[test_case]
fn test_alignment_small_preceding_fragment() {
    unsafe {
        println!("Testing alignment with small preceding fragment...");
        setup_allocator();

        let small_layout = Layout::from_size_align(9, 8).unwrap();
        let _small_ptr = TEST_ALLOCATOR.alloc(small_layout);

        let align = 128;
        let aligned_layout = Layout::from_size_align(128, align).unwrap();
        let aligned_ptr = TEST_ALLOCATOR.alloc(aligned_layout);

        assert!(!aligned_ptr.is_null(), "Allocation failed unexpectedly");

        assert_eq!(
            aligned_ptr as usize % align,
            0,
            "Allocated pointer should be aligned to 128 bytes"
        );

        assert_heap_invariants();
    }
}

#[test_case]
fn test_alignment_with_heap_growth_extend_free_block() {
    unsafe {
        println!("Testing alignment with heap extension existing free block...");
        setup_allocator();

        let _ptr = TEST_ALLOCATOR.alloc(Layout::from_size_align(3072, 8).unwrap());

        let initial_tail = *TEST_ALLOCATOR.tail.get();

        let alloc_size = 1024;
        let align = 256;
        let layout = Layout::from_size_align(alloc_size, 256).unwrap();
        let ptr = TEST_ALLOCATOR.alloc(layout);

        assert!(!ptr.is_null(), "Allocation after growth failed");
        assert_eq!(ptr as usize % align, 0, "Allocated pointer should be aligned to 128 bytes");

        let new_tail = *TEST_ALLOCATOR.tail.get();

        assert!(new_tail != initial_tail, "Tail should have moved");

        assert_heap_invariants();
    }
}

#[test_case]
fn test_alignment_with_heap_growth_create_new_block() {
    unsafe {
        println!("Testing alignment with heap extension create new block...");
        setup_allocator();

        let initial_tail = *TEST_ALLOCATOR.tail.get();

        let full_layout = Layout::from_size_align((*initial_tail).size(), 8).unwrap();
        let _ptr = TEST_ALLOCATOR.alloc(full_layout);

        let align = 256;
        let layout = Layout::from_size_align(256, align).unwrap();
        let aligned_ptr = TEST_ALLOCATOR.alloc(layout);
        assert!(!aligned_ptr.is_null(), "Allocation after growth failed");

        let new_tail = *TEST_ALLOCATOR.tail.get();

        assert_ne!(
            new_tail, initial_tail,
            "Tail should have moved (new block should have been created)"
        );

        assert!((*new_tail).is_free(), "New tail block should be free (after growth)");

        assert!(!aligned_ptr.is_null(), "Allocation failed unexpectedly");
        assert_eq!(
            aligned_ptr as usize % align,
            0,
            "Allocated pointer should be aligned to 128 bytes"
        );

        assert_heap_invariants();
    }
}

/// Regression test for split_block overflow when an aligned allocation fits
/// the check_aligned_fit condition exactly but leaves no room for the trailing
/// block header.
///
/// Setup: allocating 3952 bytes (8-aligned) leaves a 112-byte tail free block
/// whose alloc area starts at H+3984 (≡ 16 mod 64). A subsequent 64-byte /
/// 64-aligned allocation finds the aligned address at H+4032, so:
///
///   check_aligned_fit passes:  align_up(H+4032+64, 8) = H+4096 <= end (H+4096)
///   split_block excess:        112 - (32+16) - (64+16) = -16  ← overflow
///
/// After the fix, check_aligned_fit rejects this slot (needs BLOCK_HEADER_SIZE
/// more room), the allocator grows the heap, and the allocation succeeds.
#[test_case]
fn test_aligned_alloc_no_room_for_trailing_header() {
    unsafe {
        println!("Testing aligned allocation with no room for trailing block header...");
        setup_allocator();

        let _setup = TEST_ALLOCATOR.alloc(Layout::from_size_align(3952, 8).unwrap());
        assert!(!_setup.is_null(), "Setup allocation failed");

        let layout = Layout::from_size_align(64, 64).unwrap();
        let ptr = TEST_ALLOCATOR.alloc(layout);

        assert!(!ptr.is_null(), "Aligned allocation failed");
        assert_eq!(ptr as usize % 64, 0, "Allocation not aligned to 64 bytes");

        assert_heap_invariants();
    }
}

unsafe fn setup_allocator() {
    TEST_ALLOCATOR.init(core::ptr::addr_of!(TEST_HEAP) as usize, TEST_HEAP_SIZE / 2);
}

fn test_grow_heap(size: usize) -> Option<(usize, usize)> {
    let heap_start = core::ptr::addr_of!(TEST_HEAP) as usize;
    let second_half_start = heap_start + (TEST_HEAP_SIZE / 2);
    let second_half_size = TEST_HEAP_SIZE / 2;

    if size > second_half_size {
        None
    } else {
        Some((second_half_start, second_half_size))
    }
}

unsafe fn setup_fragmented_heap_for_test() -> (*mut u8, usize) {
    setup_allocator();

    let small_layout = Layout::from_size_align(128, 8).unwrap();
    let small_ptr = TEST_ALLOCATOR.alloc(small_layout);

    let remaining_block = (**TEST_ALLOCATOR.head.get()).next();

    let remaining_size = (*remaining_block).size();

    let big_layout = Layout::from_size_align(remaining_size, 8).unwrap();
    let _big_ptr = TEST_ALLOCATOR.alloc(big_layout);

    TEST_ALLOCATOR.dealloc(small_ptr, small_layout);

    (small_ptr, 128) // Return the freed block's ptr and size for the test
}

unsafe fn assert_heap_invariants() {
    let mut current = *TEST_ALLOCATOR.head.get();
    let mut last_block_addr = null_mut();

    loop {
        assert_eq!(current as usize % 8, 0, "Block address not properly aligned: {:p}", current);

        assert!(
            (*current).size() >= MIN_ALLOC_SIZE,
            "Block size must be greater than MIN_ALLOC_SIZE"
        );

        assert!(current > last_block_addr, "Block addresses not strictly increasing");

        let next_ptr = (*current).next();
        if next_ptr != *TEST_ALLOCATOR.heap_end.get() {
            let next_prev_ptr = (*next_ptr).prev();
            assert_eq!(
                next_prev_ptr, current,
                "Next block's prev does not point back to current block"
            );
        }

        let prev_ptr = (*current).prev();
        if !prev_ptr.is_null() {
            let prev_next_ptr = (*prev_ptr).next();
            assert_eq!(
                prev_next_ptr, current,
                "Prev block's next does not point forward to current block"
            );
        }

        if !next_ptr.is_null() {
            assert_eq!(
                next_ptr as *mut BlockHeader,
                ((*current).size() as usize + BLOCK_HEADER_SIZE + current as usize)
                    as *mut BlockHeader,
                "Next block address does not match expected address based on current block size"
            );
        }

        last_block_addr = current;

        current = (*current).next();

        if current == *TEST_ALLOCATOR.heap_end.get() {
            break;
        }
    }

    let tail_block = *TEST_ALLOCATOR.tail.get();

    assert_eq!(last_block_addr, tail_block, "Allocator tail does not point to last block in heap");

    let mut free_current = *TEST_ALLOCATOR.free_head.get();

    while !free_current.is_null() {
        assert!((*free_current).is_free(), "Free block is marked used: {:p}", free_current);

        let next_free_block = (*free_current).free_next();
        if !next_free_block.is_null() {
            let next_free_prev = (*next_free_block).free_prev();
            assert_eq!(
                next_free_prev, free_current,
                "Next free block's free_prev does not point back to current free block"
            );
        }

        let prev_free_block = (*free_current).free_prev();
        if !prev_free_block.is_null() {
            let prev_free_next = (*prev_free_block).free_next();
            assert_eq!(
                prev_free_next, free_current,
                "Prev free block's free_next does not point forward to current free block"
            );
        }

        free_current = (*free_current).free_next();
    }

    let mut mem_current = *TEST_ALLOCATOR.head.get();

    loop {
        if (*mem_current).is_free() {
            let mut found_in_free_list = false;

            let mut free_current = *TEST_ALLOCATOR.free_head.get();
            while !free_current.is_null() {
                if free_current == mem_current {
                    found_in_free_list = true;
                    break;
                }
                free_current = (*free_current).free_next();
            }

            assert!(
                found_in_free_list,
                "Block {:p} marked free but not found in free list",
                mem_current
            );
        }

        mem_current = (*mem_current).next();

        if mem_current == *TEST_ALLOCATOR.heap_end.get() {
            break;
        }
    }
}
