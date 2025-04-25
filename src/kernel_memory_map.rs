use core::mem::MaybeUninit;

use page_mapper::PageFlags;

use crate::dtb;
use crate::memory;
use crate::page_mapper;

const HIGH_HALF_PHYS_START: usize = 0xffff_ffff_0000_0000;

pub const KERNEL_STACK_START: usize = HIGH_HALF_PHYS_START + 0xffe0_0000;
const KERNEL_STACK_STARTING_SIZE: usize = 0x4000;
static mut CURRENT_KERNEL_STACK_END: usize = 0;
const KERNEL_STACK_END: usize = KERNEL_STACK_START - 0x20_0000;

#[no_mangle]
pub static KERNEL_TRAP_STACK_START: usize = HIGH_HALF_PHYS_START + 0xffff_c000;
const KERNEL_TRAP_STACK_SIZE: usize = 0x4000;

static mut KERNEL_PAGE_MAPPER: MaybeUninit<page_mapper::PageMapper> = MaybeUninit::uninit();

pub fn init(memory: memory::Region) -> *const page_mapper::PageTable {
    init_page_mapper();

    // Identity map the actual physical memory range
    with_page_mapper(|mapper| {
        mapper.map_range(
            memory.start,
            memory.start,
            memory.end,
            PageFlags::READ
                | PageFlags::WRITE
                | PageFlags::EXECUTE
                | PageFlags::ACCESSED
                | PageFlags::DIRTY,
            page_mapper::PageSize::Size2M,
        );
    });

    // Map the high half kernel text segment
    extern "C" {
        static _text_start: u8;
        static _text_end: u8;
    }

    let text_start = unsafe { &_text_start as *const u8 as usize };
    let phys_text_start = unsafe { &_text_start as *const u8 as usize - HIGH_HALF_PHYS_START };
    let text_end = unsafe { memory::align_up(&_text_end as *const u8 as usize) };

    println!(
        "Mapping kernel text segment: virt: {:#x} - {:#x} to phys: {:#x}",
        text_start, text_end, phys_text_start
    );

    with_page_mapper(|mapper| {
        mapper.map_range(
            text_start,
            phys_text_start,
            text_end,
            PageFlags::READ | PageFlags::EXECUTE | PageFlags::ACCESSED,
            page_mapper::PageSize::Size4K,
        );
    });

    // Map the high half kernel rodata segment
    extern "C" {
        static _rodata_start: u8;
        static _rodata_end: u8;
    }

    let rodata_start = unsafe { &_rodata_start as *const u8 as usize };
    let phys_rodata_start = unsafe { &_rodata_start as *const u8 as usize - HIGH_HALF_PHYS_START };
    let rodata_end = unsafe { memory::align_up(&_rodata_end as *const u8 as usize) };

    println!(
        "Mapping kernel rodata segment: virt: {:#x} - {:#x} to phys: {:#x}",
        rodata_start, rodata_end, phys_rodata_start
    );

    with_page_mapper(|mapper| {
        mapper.map_range(
            rodata_start,
            phys_rodata_start,
            rodata_end,
            PageFlags::READ | PageFlags::ACCESSED,
            page_mapper::PageSize::Size4K,
        );
    });

    // Map the high half kernel data segment
    extern "C" {
        static _data_start: u8;
        static _data_end: u8;
    }

    let data_start = unsafe { &_data_start as *const u8 as usize };
    let phys_data_start = unsafe { &_data_start as *const u8 as usize - HIGH_HALF_PHYS_START };
    let data_end = unsafe { memory::align_up(&_data_end as *const u8 as usize) };

    println!(
        "Mapping kernel data segment: virt: {:#x} - {:#x} to phys: {:#x}",
        data_start, data_end, phys_data_start
    );

    with_page_mapper(|mapper| {
        mapper.map_range(
            data_start,
            phys_data_start,
            data_end,
            PageFlags::READ | PageFlags::WRITE | PageFlags::ACCESSED | PageFlags::DIRTY,
            page_mapper::PageSize::Size4K,
        );
    });

    // Map the high half kernel bss segment
    extern "C" {
        static _bss_start: u8;
        static _bss_end: u8;
    }

    let bss_start = unsafe { &_bss_start as *const u8 as usize };
    let phys_bss_start = unsafe { &_bss_start as *const u8 as usize - HIGH_HALF_PHYS_START };
    let bss_end = unsafe { memory::align_up(&_bss_end as *const u8 as usize) };

    println!(
        "Mapping kernel bss segment: virt: {:#x} - {:#x} to phys: {:#x}",
        bss_start, bss_end, phys_bss_start
    );

    with_page_mapper(|mapper| {
        mapper.map_range(
            bss_start,
            phys_bss_start,
            bss_end,
            PageFlags::READ | PageFlags::WRITE | PageFlags::ACCESSED | PageFlags::DIRTY,
            page_mapper::PageSize::Size4K,
        );
    });

    // Map the high half kernel stack segment
    let kernel_stack_end = KERNEL_STACK_START - KERNEL_STACK_STARTING_SIZE;
    println!(
        "Mapping kernel stack segment: virt: {:#x} - {:#x}",
        kernel_stack_end, KERNEL_STACK_START
    );

    with_page_mapper(|mapper| {
        mapper.allocate_and_map_pages(
            kernel_stack_end,
            KERNEL_STACK_STARTING_SIZE,
            PageFlags::READ | PageFlags::WRITE | PageFlags::ACCESSED | PageFlags::DIRTY,
        );
    });

    unsafe {
        CURRENT_KERNEL_STACK_END = kernel_stack_end;
    }

    // Map the high half kernel trap stack segment
    println!(
        "Mapping kernel trap stack segment: virt: {:#x} - {:#x}",
        KERNEL_TRAP_STACK_START - KERNEL_TRAP_STACK_SIZE,
        KERNEL_TRAP_STACK_START
    );

    with_page_mapper(|mapper| {
        mapper.allocate_and_map_pages(
            KERNEL_TRAP_STACK_START - KERNEL_TRAP_STACK_SIZE,
            KERNEL_TRAP_STACK_SIZE,
            PageFlags::READ | PageFlags::WRITE | PageFlags::ACCESSED | PageFlags::DIRTY,
        );
    });

    let root_table = root_table();
    println!("Memory mapped at root table: {:#x}", root_table as usize);

    root_table
}

pub fn grow_stack_on_page_fault(fault_address: usize) -> bool {
    const STACK_GROWTH_CHUNK: usize = 0x4000;

    if !(KERNEL_STACK_END..=KERNEL_STACK_START).contains(&fault_address) {
        return false;
    }

    let aligned_fault_address = memory::align_down(fault_address);
    let current_stack_end = unsafe { CURRENT_KERNEL_STACK_END };

    if aligned_fault_address < current_stack_end - STACK_GROWTH_CHUNK {
        return false;
    }

    let grow_end = (aligned_fault_address - STACK_GROWTH_CHUNK).max(KERNEL_STACK_END);

    println!("Growing kernel stack: virt: {:#x} - {:#x}", grow_end, current_stack_end,);

    with_page_mapper(|mapper| {
        mapper.allocate_and_map_pages(
            grow_end,
            current_stack_end - grow_end,
            PageFlags::READ | PageFlags::WRITE | PageFlags::ACCESSED | PageFlags::DIRTY,
        );
    });

    if dtb::get_cpu_type() == dtb::CpuType::LicheeRVNano {
        unsafe {
            core::arch::asm!(
                ".long 0x0020000b",
                ".long 0x0190000b",
                options(nostack, preserves_flags),
            );
        }
    }

    unsafe {
        core::arch::asm!("sfence.vma zero, zero", options(nostack, preserves_flags),);
    }

    unsafe {
        CURRENT_KERNEL_STACK_END = grow_end;
    }

    println!("Kernel stack grown to: {:#x}", grow_end);

    true
}

pub fn init_page_mapper() {
    let mapper = page_mapper::PageMapper::new();
    unsafe {
        KERNEL_PAGE_MAPPER.write(mapper);
    }
}

fn kernel_page_mapper_ref() -> &'static page_mapper::PageMapper {
    unsafe { KERNEL_PAGE_MAPPER.assume_init_ref() }
}

fn root_table() -> *const page_mapper::PageTable {
    kernel_page_mapper_ref().root_table
}

fn with_page_mapper<F>(f: F)
where
    F: FnOnce(&page_mapper::PageMapper),
{
    f(kernel_page_mapper_ref())
}

#[allow(dead_code)]
pub fn dump_stack_pte() {
    kernel_page_mapper_ref().dump_pte(KERNEL_STACK_START - 0x1000);
}

#[allow(dead_code)]
pub fn dump_vmmap() {
    kernel_page_mapper_ref().dump_vmmap();
}
