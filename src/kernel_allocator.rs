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
struct BlockHeader {
    size: usize,
    used: bool,
    next: Option<&'static mut BlockHeader>,
    prev: Option<&'static mut BlockHeader>,
    free_next: Option<&'static mut BlockHeader>,
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
}

impl LinkedListAllocator {
    pub const fn new() -> Self {
        LinkedListAllocator {
            head: UnsafeCell::new(None),
            tail: UnsafeCell::new(None),
            free_head: UnsafeCell::new(None),
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
        };
        *self.head.get() = Some(&mut *block);
        *self.tail.get() = Some(&mut *block);
    }

    unsafe fn find_fit(&self, layout: Layout) -> Option<&'static mut BlockHeader> {
        let mut current = match (*self.head.get()).as_mut() {
            Some(block) => *block as *mut BlockHeader,
            None => null_mut(),
        };
        let mut last: *mut BlockHeader = null_mut();

        while !current.is_null() {
            let curr = &mut *current;

            if !curr.used && curr.size >= layout.size() {
                return Some(curr);
            }

            last = current;
            current = match curr.next.as_mut() {
                Some(next_block) => *next_block as *mut BlockHeader,
                None => null_mut(),
            };
        }

        let size = (layout.size() + size_of::<BlockHeader>()).max(memory::PAGE_SIZE);
        let (new_addr, actual_size) = kernel_memory_map::grow_kernel_heap(size)?;
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
            let new_block_addr = block.start_ptr().add(total_needed) as *mut BlockHeader;
            let new_block = &mut *new_block_addr;

            *new_block = BlockHeader {
                size: excess - size_of::<BlockHeader>(),
                used: false,
                next: block.next.take(),
                prev: Some(&mut *(block as *mut BlockHeader)),
                free_next: None,
            };

            let new_block_ptr: *mut BlockHeader = new_block as *mut _;

            if let Some(next_block) = new_block.next.as_mut() {
                next_block.prev = Some(unsafe { &mut *new_block_ptr });
            } else {
                *self.tail.get() = Some(unsafe { &mut *new_block_ptr });
            }

            block.size = total_needed;
            block.next = Some(&mut *new_block_addr);
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
                "  Next: {:?}, Prev: {:?}, Free Next: {:?}",
                block.next.as_ref().map(|b| *b as *const _),
                block.prev.as_ref().map(|b| *b as *const _),
                block.free_next.as_ref().map(|b| *b as *const _),
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
