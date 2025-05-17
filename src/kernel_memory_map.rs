use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

use page_mapper::PageFlags;

use crate::dtb;
use crate::kernel_allocator;
use crate::memory;
use crate::page_mapper;
use crate::trap;

const HIGH_HALF_PHYS_START: usize = 0xffff_ffff_0000_0000;

#[no_mangle]
pub static KERNEL_STACK_START: usize = HIGH_HALF_PHYS_START + 0xffe0_0000;

const KERNEL_STACK_STARTING_SIZE: usize = 0x4000;
static mut CURRENT_KERNEL_STACK_END: usize = 0;
const KERNEL_STACK_END: usize = KERNEL_STACK_START - 0x20_0000;

#[no_mangle]
pub static TRAP_FRAME: usize = HIGH_HALF_PHYS_START + 0xffe0_0000;
const TRAP_FRAME_SIZE: usize = 0x1000;

pub static mut LAST_L1_PTE: AtomicUsize = AtomicUsize::new(0);

pub const KERNEL_HEAP_START: usize = 0xffff_ffff_c000_0000;
pub const KERNEL_HEAP_SIZE: usize = 4 * memory::PAGE_SIZE;
static KERNEL_HEAP_END: AtomicUsize = AtomicUsize::new(KERNEL_HEAP_START + KERNEL_HEAP_SIZE);

static mut KERNEL_PAGE_MAPPER: MaybeUninit<page_mapper::PageMapper> = MaybeUninit::uninit();

pub fn init(memory: memory::Region) {
    init_page_mapper();

    identity_map_memory(memory);

    map_kernel_segments();

    map_kernel_stack();

    map_and_initialize_kernel_heap();

    map_last_l1_pte();

    switch_to_kernel_map();

    unsafe {
        kernel_allocator::ALLOCATOR.init(
            KERNEL_HEAP_START,
            KERNEL_HEAP_SIZE,
        );
    }
}

fn identity_map_memory(memory: memory::Region) {
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
}

fn map_kernel_segments() {
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
}

fn map_kernel_stack() {
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
}

fn map_and_initialize_kernel_heap() {
    println!(
        "Mapping kernel heap segment: virt: {:#x} - {:#x}",
        KERNEL_HEAP_START, KERNEL_HEAP_END.load(Ordering::SeqCst)
    );

    with_page_mapper(|mapper| {
        mapper.allocate_and_map_pages(
            KERNEL_HEAP_START,
            KERNEL_HEAP_SIZE,
            PageFlags::READ | PageFlags::WRITE | PageFlags::ACCESSED | PageFlags::DIRTY,
        );
    });
}

fn map_last_l1_pte() {
    // Map the high half process trampoline segment
    unsafe extern "C" {
        static _proc_tramp_start: u8;
        static _proc_tramp_lma: usize;
        static _proc_tramp_end: u8;
    }

    let proc_tramp_start = unsafe { &_proc_tramp_start as *const u8 as usize };
    let phys_proc_tramp_start = unsafe { _proc_tramp_lma };
    let proc_tramp_end = unsafe { memory::align_up(&_proc_tramp_end as *const u8 as usize) };

    println!(
        "Mapping kernel process trampoline segment: virt: {:#x} - {:#x} to phys: {:#x}",
        proc_tramp_start, proc_tramp_end, phys_proc_tramp_start
    );

    with_page_mapper(|mapper| {
        mapper.map_range(
            proc_tramp_start,
            phys_proc_tramp_start,
            proc_tramp_end,
            PageFlags::READ | PageFlags::EXECUTE | PageFlags::ACCESSED | PageFlags::GLOBAL,
            page_mapper::PageSize::Size4K,
        );
    });

    // Map the high half kernel trap frame segment
    println!(
        "Mapping kernel trap trap segment: virt: {:#x} - {:#x}",
        TRAP_FRAME,
        TRAP_FRAME + TRAP_FRAME_SIZE,
    );

    with_page_mapper(|mapper| {
        mapper.allocate_and_map_pages(
            TRAP_FRAME,
            TRAP_FRAME_SIZE,
            PageFlags::READ | PageFlags::WRITE | PageFlags::ACCESSED | PageFlags::DIRTY,
        );
    });
    let last_l1_pte = kernel_page_mapper_ref().l1_page_table_from_phys(TRAP_FRAME);
    println!("mapped last l1 pte, pte is: {:#x}", last_l1_pte as usize);
    unsafe {
        LAST_L1_PTE.store(last_l1_pte as usize, Ordering::SeqCst);
    }
}

#[inline(always)]
pub fn switch_to_kernel_map() {
    let root_table = root_table();
    println!("Switching to kernel map with root table: {:#x}", root_table as usize);

    let ppn = root_table as usize >> 12;
    let satp_value = (8 << 60) | ppn;

    println!("Switching to memory map with SATP value: {:#x}", satp_value);

    unsafe {
        core::arch::asm!(
            "csrw satp, {0}",
            in(reg) satp_value,
            options(nostack)
        );

        lichee_clear_cache();

        core::arch::asm!(
            "sfence.vma zero, zero",
            options(nostack)
        );
    }

    let trap_frame = TRAP_FRAME as *mut trap::TrapFrame;
    unsafe {
        (*trap_frame).kernel_satp = satp_value;
        (*trap_frame).is_lichee_rvnano = (dtb::get_cpu_type() == dtb::CpuType::LicheeRVNano) as usize;
        println!("is_lichee_rvnano: {}", (*trap_frame).is_lichee_rvnano);
    }

    println!("Switched to kernel map");
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

pub fn grow_kernel_heap(size: usize) -> Option<(usize, usize)> {
    println!("Growing kernel heap by: {:#x}", size);
    let size = memory::align_up(size);

    let old_end = KERNEL_HEAP_END.load(Ordering::SeqCst);
    let new_end = old_end.checked_add(size)?;

    println!(
        "Growing kernel heap: virt: {:#x} - {:#x}",
        old_end, new_end
    );

    with_page_mapper(|mapper| {
        mapper.allocate_and_map_pages(
            old_end,
            size,
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

    KERNEL_HEAP_END.store(new_end, Ordering::SeqCst);

    Some((old_end, size))
}

#[inline]
fn lichee_clear_cache() {
    if dtb::get_cpu_type() == dtb::CpuType::LicheeRVNano {
        unsafe {
            core::arch::asm!(
                ".long 0x0020000b",
                ".long 0x0190000b",
                options(nostack, preserves_flags),
            );
        }
    }
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
