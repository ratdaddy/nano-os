#![allow(dead_code)]

use crate::memory;

#[repr(C)]
struct PageNode {
    next: Option<&'static mut PageNode>,
}

pub struct PageAllocator {
    head: Option<&'static mut PageNode>,
    free_pages: usize,
    total_pages: usize,
}

impl PageAllocator {
    pub const fn new() -> Self {
        Self { head: None, free_pages: 0, total_pages: 0 }
    }

    pub unsafe fn init<const N: usize>(&mut self, usable_memory: &heapless::Vec<memory::Region, N>) {
        self.head = None;
        self.free_pages = 0;

        for region in usable_memory.iter() {
            println!("Page allocator initializing from {:#x} to {:#x}", region.start, region.end);
            assert!(region.start & (memory::PAGE_SIZE - 1) == 0, "Page allocator start address not page-aligned.");
            assert!(region.end & (memory::PAGE_SIZE - 1) == 0, "Page allocator end address not page-aligned.");
            let mut addr = region.start;
            while addr < region.end {
                self.dealloc(addr);
                addr += memory::PAGE_SIZE;
            }
        }
        self.total_pages = self.free_pages;
    }

    pub fn alloc(&mut self) -> Option<usize> {
        self.free_pages -= 1;
        self.head.take().map(|node| {
            self.head = node.next.take();
            println!("Allocated page at {:#x}", node as *mut PageNode as usize);
            node as *mut PageNode as usize
        })
    }

    pub fn dealloc(&mut self, ptr: usize) {
        self.free_pages += 1;
        unsafe {
            let node = ptr as *mut PageNode;
            (*node).next = self.head.take();
            self.head = Some(&mut *node);
        }
    }

    pub fn free_page_count(&self) -> usize {
        self.free_pages
    }

    pub fn total_page_count(&self) -> usize {
        self.total_pages
    }
}

use core::ptr::addr_of_mut;

static mut PAGE_ALLOCATOR: PageAllocator = PageAllocator::new();

pub unsafe fn init<const N: usize>(usable_memory: &heapless::Vec<memory::Region, N>) {
    (*addr_of_mut!(PAGE_ALLOCATOR)).init(usable_memory);
}

pub fn alloc() -> Option<usize> {
    unsafe { (*addr_of_mut!(PAGE_ALLOCATOR)).alloc() }
}

pub fn dealloc(ptr: usize) {
    unsafe { (*addr_of_mut!(PAGE_ALLOCATOR)).dealloc(ptr) }
}

pub fn free_page_count() -> usize {
    unsafe { (*addr_of_mut!(PAGE_ALLOCATOR)).free_page_count() }
}

pub fn total_page_count() -> usize {
    unsafe { (*addr_of_mut!(PAGE_ALLOCATOR)).total_page_count() }
}
