use crate::memory;
use crate::page_mapper;

pub fn init(memory: memory::Region) -> *mut page_mapper::PageTable {
    let page_mapper = page_mapper::PageMapper::new();

    use page_mapper::PageFlags;

    // Identity map the actual physical memory range
    page_mapper.map_range(
        memory.start,
        memory.start,
        memory.end,
        PageFlags::READ.union(PageFlags::WRITE).union(PageFlags::EXECUTE).union(PageFlags::ACCESSED).union(PageFlags::DIRTY),
        page_mapper::PageSize::Size2M,
    );

    let high_half_phys_start = 0xffffffff00000000;

    // Map the high half kernel text segment
    extern "C" {
        static _text_start: u8;
        static _text_end: u8;
    }

    let text_start = unsafe { &_text_start as *const u8 as usize };
    let phys_text_start = unsafe { &_text_start as *const u8 as usize - high_half_phys_start };
    let text_end = unsafe { memory::align_up(&_text_end as *const u8 as usize) };

    println!("Mapping kernel text segment: virt: {:#x} - {:#x} to phys: {:#x}",
             text_start, text_end, phys_text_start);

    page_mapper.map_range(
        text_start,
        phys_text_start,
        text_end,
        PageFlags::READ.union(PageFlags::EXECUTE).union(PageFlags::ACCESSED),
        page_mapper::PageSize::Size4K,
    );

    // Map the high half kernel rodata segment
    extern "C" {
        static _rodata_start: u8;
        static _rodata_end: u8;
    }

    let rodata_start = unsafe { &_rodata_start as *const u8 as usize };
    let phys_rodata_start = unsafe { &_rodata_start as *const u8 as usize - high_half_phys_start };
    let rodata_end = unsafe { memory::align_up(&_rodata_end as *const u8 as usize) };

    println!("Mapping kernel rodata segment: virt: {:#x} - {:#x} to phys: {:#x}",
             rodata_start, rodata_end, phys_rodata_start);

    page_mapper.map_range(
        rodata_start,
        phys_rodata_start,
        rodata_end,
        PageFlags::READ.union(PageFlags::ACCESSED),
        page_mapper::PageSize::Size4K,
    );

    // Map the high half kernel data segment
    extern "C" {
        static _data_start: u8;
        static _data_end: u8;
    }

    let data_start = unsafe { &_data_start as *const u8 as usize };
    let phys_data_start = unsafe { &_data_start as *const u8 as usize - high_half_phys_start };
    let data_end = unsafe { memory::align_up(&_data_end as *const u8 as usize) };

    println!("Mapping kernel data segment: virt: {:#x} - {:#x} to phys: {:#x}",
             data_start, data_end, phys_data_start);

    page_mapper.map_range(
        data_start,
        phys_data_start,
        data_end,
        PageFlags::READ.union(PageFlags::WRITE).union(PageFlags::ACCESSED).union(PageFlags::DIRTY),
        page_mapper::PageSize::Size4K,
    );

    // Map the high half kernel bss segment
    extern "C" {
        static _bss_start: u8;
        static _bss_end: u8;
    }

    let bss_start = unsafe { &_bss_start as *const u8 as usize };
    let phys_bss_start = unsafe { &_bss_start as *const u8 as usize - high_half_phys_start };
    let bss_end = unsafe { memory::align_up(&_bss_end as *const u8 as usize) };

    println!("Mapping kernel bss segment: virt: {:#x} - {:#x} to phys: {:#x}",
             bss_start, bss_end, phys_bss_start);

    page_mapper.map_range(
        bss_start,
        phys_bss_start,
        bss_end,
        PageFlags::READ.union(PageFlags::WRITE).union(PageFlags::ACCESSED).union(PageFlags::DIRTY),
        page_mapper::PageSize::Size4K,
    );

    println!("Memory mapped at root table: {:#x}", page_mapper.root_table as usize);

    page_mapper.root_table
}
