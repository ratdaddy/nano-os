use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
use core::mem::size_of;
use core::cell::UnsafeCell;

use crate::kernel_memory_map;
use crate::memory;

const BLOCK_HEADER_SIZE: usize = size_of::<BlockHeader>();
const MIN_ALLOC_SIZE: usize = 16;
const MIN_BLOCK_SIZE: usize = BLOCK_HEADER_SIZE + MIN_ALLOC_SIZE;

const NULL_OFFSET: u32 = 0xffff_fffe;
const OFFSET_LOWER_BITS_MASK: u32 = !NULL_OFFSET;

#[global_allocator]
pub static ALLOCATOR: LinkedListAllocator = LinkedListAllocator::new();

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

#[repr(C)]
#[derive(Debug)]
struct BlockHeader {
    _size: u32,
    _prev: u32,
    _free_next: u32,
    _free_prev: u32,
}

impl BlockHeader {
    fn new(size: usize, used: bool) -> Self {
        BlockHeader {
            _size: size as u32,
            _prev: NULL_OFFSET,
            _free_next: NULL_OFFSET,
            _free_prev: NULL_OFFSET | if used { 1 } else { 0 },
        }
    }

    #[inline]
    fn alloc_area_start(&self) -> *mut u8 {
        unsafe {
            (self as *const _ as *mut u8).add(BLOCK_HEADER_SIZE)
        }
    }

    #[inline]
    fn next(&self) -> *mut BlockHeader {
        let this = self as *const BlockHeader;
        (this as usize + self._size as usize + BLOCK_HEADER_SIZE) as *mut BlockHeader
    }

    #[inline]
    fn prev(&self) -> *mut BlockHeader {
        from_offset(self._prev)
    }

    #[inline]
    fn set_prev(&mut self, prev: *mut BlockHeader) {
        self._prev = to_offset(prev);
    }

    #[inline]
    fn free_next(&self) -> *mut BlockHeader {
        from_offset(self._free_next)
    }

    #[inline]
    fn set_free_next(&mut self, next: *mut BlockHeader) {
        self._free_next = to_offset(next);
    }

    #[inline]
    fn free_prev(&self) -> *mut BlockHeader {
        let free_prev_offset = self._free_prev & NULL_OFFSET;

        if free_prev_offset == NULL_OFFSET {
            null_mut()
        } else {
            (heap_start() + free_prev_offset as usize) as *mut BlockHeader
        }
    }

    #[inline]
    fn set_free_prev(&mut self, prev: *mut BlockHeader) {
        self._free_prev &= OFFSET_LOWER_BITS_MASK;

        if prev.is_null() {
            self._free_prev |= NULL_OFFSET;
        } else {
            self._free_prev |= (prev as *const _ as usize - heap_start()) as u32
        }
    }

    #[inline]
    fn size(&self) -> usize {
        self._size as usize
    }

    #[inline]
    fn set_size(&mut self, size: usize) {
        self._size = size as u32;
    }

    #[inline]
    fn add_size(&mut self, size: usize) {
        self._size += size as u32;
    }

    #[inline]
    fn is_used(&self) -> bool {
        (self._free_prev & 1) != 0
    }

    #[inline]
    fn set_used(&mut self) {
        self._free_prev |= 1;
    }

    #[inline]
    fn is_free(&self) -> bool {
        (self._free_prev & 1) == 0
    }

    #[inline]
    fn set_free(&mut self) {
        self._free_prev &= !1;
    }

    #[inline]
    fn end_ptr(&self) -> usize {
        self as *const _ as usize + BLOCK_HEADER_SIZE + self._size as usize
    }
}

#[cfg(test)]
const TEST_HEAP_SIZE: usize = memory::PAGE_SIZE * 2;

#[cfg(test)]
#[repr(align(4096))]
struct AlignedHeap([u8; TEST_HEAP_SIZE]);

#[cfg(test)]
static mut TEST_HEAP: AlignedHeap = AlignedHeap([0; TEST_HEAP_SIZE]);

#[cfg(test)]
#[inline]
fn heap_start() -> usize {
    unsafe { TEST_HEAP.0.as_ptr() as usize }
}

#[cfg(not(test))]
#[inline]
fn heap_start() -> usize {
        kernel_memory_map::KERNEL_HEAP_START
}

#[inline]
fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}


#[inline]
fn from_offset(offset: u32) -> *mut BlockHeader {
    if offset == NULL_OFFSET {
        null_mut()
    } else {
        (heap_start() + offset as usize) as *mut BlockHeader
    }
}

#[inline]
fn to_offset(pointer: *mut BlockHeader) -> u32 {
    if pointer.is_null() {
        NULL_OFFSET
    } else {
        (pointer as *const _ as usize - heap_start()) as u32
    }
}

pub struct LinkedListAllocator {
    head: UnsafeCell<*mut BlockHeader>,
    tail: UnsafeCell<*mut BlockHeader>,
    heap_end: UnsafeCell<*mut BlockHeader>,
    free_head: UnsafeCell<*mut BlockHeader>,
    grow_heap_fn: fn(usize) -> Option<(usize, usize)>,
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
            let aligned_location = check_aligned_fit(last, size, align).expect("Failed to find aligned fit");
            return Some((last, aligned_location));
        } else {
            let this = new_heap as *mut BlockHeader;
            *this = BlockHeader::new(actual_size - BLOCK_HEADER_SIZE, false);

            self.insert_free_block(this);

            self.append_to_list(this);

            let aligned_location = check_aligned_fit(this, size, align).expect("Failed to find aligned fit");
            return Some((this, aligned_location));
        }
    }

    unsafe fn split_block(&self, this: *mut BlockHeader, aligned_block: *mut BlockHeader, layout: Layout) -> *mut u8 {
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
            let excess = this_size - (preceding_size + BLOCK_HEADER_SIZE) - (aligned_block_size + BLOCK_HEADER_SIZE);

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
        println!(" Head: {:?}, Free head: {:?}",
            *self.head.get(),
            *self.free_head.get(),
        );

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
            println!(
                "  Next: {:?}, Prev: {:?}",
                (*current).next(),
                (*current).prev(),
            );
            println!("  Free Next: {:?}, Free Prev: {:?}",
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

#[cfg(test)]
mod tests {
    use super::*;
    use core::alloc::Layout;

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
            assert!((*this).size() + BLOCK_HEADER_SIZE == MIN_BLOCK_SIZE, "Allocated block should be minimum size");

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

            let initial_tail_size = (*initial_tail).size();

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

            assert_eq!(
                free_head,
                head,
                "Free head should point to initial free block"
            );

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
                    free_current,
                    original_free_head,
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
            let block1 = TEST_ALLOCATOR.alloc(layout);
            let block2 = TEST_ALLOCATOR.alloc(layout);
            let block3 = TEST_ALLOCATOR.alloc(layout);

            TEST_ALLOCATOR.dealloc(block2, layout);

            let header2 = (block2 as usize - BLOCK_HEADER_SIZE) as *mut BlockHeader;

            assert_eq!((*header2).size(), 128, "Freed block should retain its size after no coalescing");
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
            let block1 = TEST_ALLOCATOR.alloc(layout);
            let block2 = TEST_ALLOCATOR.alloc(layout);
            let block3 = TEST_ALLOCATOR.alloc(layout);
            let block4 = TEST_ALLOCATOR.alloc(layout);

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
            let block3 = TEST_ALLOCATOR.alloc(layout);

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
            let block4 = TEST_ALLOCATOR.alloc(layout);

            TEST_ALLOCATOR.dealloc(block3, layout);
            TEST_ALLOCATOR.dealloc(block1, layout);
            TEST_ALLOCATOR.dealloc(block2, layout);

            let header1 = (block1 as usize - BLOCK_HEADER_SIZE) as *mut BlockHeader;

            let expected_total_size = 128
                + BLOCK_HEADER_SIZE
                + 128
                + BLOCK_HEADER_SIZE
                + 128;

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

            assert_eq!( (*header1).size(), original_size, "All blocks should have merged into one"
            );

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
            let small_ptr = TEST_ALLOCATOR.alloc(small_layout);

            let align = 128;
            let aligned_layout = Layout::from_size_align(128, align).unwrap();
            let aligned_ptr = TEST_ALLOCATOR.alloc(aligned_layout);

            assert!(!aligned_ptr.is_null(), "Allocation failed unexpectedly");

            assert_eq!(aligned_ptr as usize % align, 0, "Allocated pointer should be aligned to 128 bytes");

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
            let ptr = TEST_ALLOCATOR.alloc(full_layout);

            let align = 256;
            let layout = Layout::from_size_align(256, align).unwrap();
            let aligned_ptr = TEST_ALLOCATOR.alloc(layout);
            assert!(!aligned_ptr.is_null(), "Allocation after growth failed");

            let new_tail = *TEST_ALLOCATOR.tail.get();

            assert_ne!(
                new_tail,
                initial_tail,
                "Tail should have moved (new block should have been created)"
            );

            assert!((*new_tail).is_free(), "New tail block should be free (after growth)");

            assert!(!aligned_ptr.is_null(), "Allocation failed unexpectedly");
            assert_eq!(aligned_ptr as usize % align, 0, "Allocated pointer should be aligned to 128 bytes");

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
        let big_ptr = TEST_ALLOCATOR.alloc(big_layout);

        TEST_ALLOCATOR.dealloc(small_ptr, small_layout);

        (small_ptr, 128) // Return the freed block's ptr and size for the test
    }

    unsafe fn assert_heap_invariants() {
        let mut current = *TEST_ALLOCATOR.head.get();
        let mut last_block_addr = null_mut();

        loop {
            assert_eq!(current as usize % 8, 0, "Block address not properly aligned: {:p}", current);

            assert!((*current).size() >= MIN_ALLOC_SIZE, "Block size must be greater than MIN_ALLOC_SIZE");

            assert!(current > last_block_addr, "Block addresses not strictly increasing");

            let next_ptr = (*current).next();
            if next_ptr != *TEST_ALLOCATOR.heap_end.get() {
                let next_prev_ptr = (*next_ptr).prev();
                assert_eq!(
                    next_prev_ptr,
                    current,
                    "Next block's prev does not point back to current block"
                );
            }

            let prev_ptr = (*current).prev();
            if !prev_ptr.is_null() {
                let prev_next_ptr = (*prev_ptr).next();
                assert_eq!(
                    prev_next_ptr,
                    current,
                    "Prev block's next does not point forward to current block"
                );
            }

            if !next_ptr.is_null() {
                assert_eq!(next_ptr as *mut BlockHeader, ((*current).size() as usize + BLOCK_HEADER_SIZE + current as usize) as *mut BlockHeader, "Next block address does not match expected address based on current block size");
            }

            last_block_addr = current;

            current = (*current).next();

            if current == *TEST_ALLOCATOR.heap_end.get() {
                break;
            }
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
                (*free_current).is_free(),
                "Free block is marked used: {:p}",
                free_current
            );

            let next_free_block = (*free_current).free_next();
            if !next_free_block.is_null() {
                let next_free_prev = (*next_free_block).free_prev();
                assert_eq!(
                    next_free_prev,
                    free_current,
                    "Next free block's free_prev does not point back to current free block"
                );
            }

            let prev_free_block = (*free_current).free_prev();
            if !prev_free_block.is_null() {
                let prev_free_next = (*prev_free_block).free_next();
                assert_eq!(
                    prev_free_next,
                    free_current,
                    "Prev free block's free_next does not point forward to current free block"
                );
            }

            last_free_addr = block_addr;
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
}
