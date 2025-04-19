use core::ptr::addr_of_mut;

use crate::dtb;
use crate::memory;

#[repr(C)]
struct PageNode {
    next: Option<&'static mut PageNode>,
    page_count: usize,
}

pub struct PageAllocator {
    head: Option<&'static mut PageNode>,
    free_pages: usize,
    total_pages: usize,
}

static mut PAGE_ALLOCATOR: PageAllocator = PageAllocator::new();

pub fn init(dtb_ptr: *const u8, kernel_phys_end: usize) -> memory::Region {
    let dtb_context = unsafe { dtb::parse_dtb(dtb_ptr) };

    const MAX_RESERVED_MEMORY: usize = 16;
    const MAX_USABLE_MEMORY: usize = MAX_RESERVED_MEMORY + 1;

    let mut reserved_memory: heapless::Vec<memory::Region, MAX_RESERVED_MEMORY> =
        heapless::Vec::new();
    let mut usable_memory: heapless::Vec<memory::Region, MAX_USABLE_MEMORY> = heapless::Vec::new();

    let memory = unsafe {
        dtb::collect_memory_map(dtb_ptr, &mut reserved_memory)
            .expect("Failed to collect memory map")
    };

    println!("Memory {:#x} - {:#x}", memory.start, memory.end);

    let _ = reserved_memory
        .push(memory::Region { start: memory.start, end: memory::align_up(kernel_phys_end) });

    let _ = reserved_memory.push(memory::Region {
        start: memory::align_down(dtb_ptr as usize),
        end: memory::align_up(dtb_ptr as usize + dtb_context.total_size),
    });

    println!("Reserved memory regions:");
    for region in reserved_memory.iter() {
        println!("  {:#x} - {:#x}", region.start, region.end);
    }

    memory::compute_usable_regions(memory, &reserved_memory, &mut usable_memory);

    println!("Usable memory regions:");
    for region in usable_memory.iter() {
        println!("  {:#x} - {:#x}", region.start, region.end);
    }

    unsafe {
        (*addr_of_mut!(PAGE_ALLOCATOR)).init(&usable_memory);
    }

    println!(
        "Page allocator initialized: {} pages ({} free)",
        total_page_count(),
        free_page_count()
    );

    memory
}

pub fn alloc() -> Option<usize> {
    unsafe { (*addr_of_mut!(PAGE_ALLOCATOR)).alloc() }
}

#[allow(dead_code)]
pub fn dealloc(ptr: usize, page_count: usize) {
    unsafe { (*addr_of_mut!(PAGE_ALLOCATOR)).dealloc(ptr, page_count) }
}

pub fn free_page_count() -> usize {
    unsafe { (*addr_of_mut!(PAGE_ALLOCATOR)).free_page_count() }
}

pub fn total_page_count() -> usize {
    unsafe { (*addr_of_mut!(PAGE_ALLOCATOR)).total_page_count() }
}

impl PageAllocator {
    pub const fn new() -> Self {
        Self { head: None, free_pages: 0, total_pages: 0 }
    }

    pub unsafe fn init<const N: usize>(
        &mut self,
        usable_memory: &heapless::Vec<memory::Region, N>,
    ) {
        self.head = None;
        self.free_pages = 0;

        for region in usable_memory.iter() {
            println!("Page allocator initializing from {:#x} to {:#x}", region.start, region.end);
            assert!(
                region.start & (memory::PAGE_SIZE - 1) == 0,
                "Page allocator start address not page-aligned."
            );
            assert!(
                region.end & (memory::PAGE_SIZE - 1) == 0,
                "Page allocator end address not page-aligned."
            );

            let start = region.start;
            let end = region.end;
            let page_count = (end - start) / memory::PAGE_SIZE;
            assert_eq!(start % memory::PAGE_SIZE, 0);
            assert_eq!(end % memory::PAGE_SIZE, 0);

            self.dealloc(start, page_count);
        }

        self.total_pages = self.free_pages;
    }

    pub fn alloc(&mut self) -> Option<usize> {
        let node_ref = self.head.as_mut()?;
        node_ref.page_count -= 1;

        let addr = Self::block_start(node_ref) + node_ref.page_count * memory::PAGE_SIZE;

        if node_ref.page_count == 0 {
            self.head = node_ref.next.take();
        }

        println!("Allocated page at {:#x}", addr as *mut PageNode as usize);

        self.free_pages -= 1;
        Some(addr)
    }

    fn block_start(node: &PageNode) -> usize {
        node as *const PageNode as usize
    }

    pub fn dealloc(&mut self, ptr: usize, page_count: usize) {
        self.free_pages += page_count;
        unsafe {
            let node = ptr as *mut PageNode;
            (*node).next = self.head.take();
            (*node).page_count = page_count;
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
