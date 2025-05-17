use crate::page_mapper::{ self, PageFlags };
use crate::kernel_memory_map;
use core::sync::atomic::Ordering;

pub const PROCESS_STACK_START: usize = 0xffe0_0000;
const PROCESS_STACK_STARTING_SIZE: usize = 0x4000;

pub fn init(page_map: &mut page_mapper::PageMapper) {
    unsafe extern "C" {
        static _proc_main_lma: usize;
    }

    let proc_main_start = 0x10000;
    let phys_proc_main_start = unsafe { _proc_main_lma };
    let proc_main_end = 0x11000;

    println!(
        "Mapping process main segment: virt: {:#x} - {:#x} to phys: {:#x}",
        proc_main_start, proc_main_end, phys_proc_main_start
    );

    page_map.map_range(
        proc_main_start,
        phys_proc_main_start,
        proc_main_end,
        PageFlags::READ | PageFlags::EXECUTE | PageFlags::ACCESSED | PageFlags::USER,
        page_mapper::PageSize::Size4K,
    );
    unsafe extern "C" {
        static _proc_tramp_start: u8;
        static _proc_tramp_lma: usize;
        static _proc_tramp_end: u8;
    }

    // Map the process stack segment
    let process_stack_end = PROCESS_STACK_START - PROCESS_STACK_STARTING_SIZE;
    println!(
        "Mapping process stack segment: virt: {:#x} - {:#x}",
        process_stack_end, PROCESS_STACK_START
    );

    page_map.allocate_and_map_pages(
        process_stack_end,
        PROCESS_STACK_STARTING_SIZE,
        PageFlags::READ | PageFlags::WRITE | PageFlags::ACCESSED | PageFlags::DIRTY | PageFlags::USER,
    );

    // Map the last l1 page table
    let last_l1_pte = unsafe {
        kernel_memory_map::LAST_L1_PTE.load(Ordering::Relaxed)
    };

    page_map.set_l1_page_table_for_phys(
        kernel_memory_map::TRAP_FRAME,
        last_l1_pte as *mut page_mapper::PageTable,
    );
}
