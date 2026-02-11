#![allow(static_mut_refs)]

use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

use page_mapper::PageFlags;

use crate::dtb;
#[cfg(not(test))]
use crate::kernel_allocator;
use crate::memory;
use crate::page_mapper;

const HIGH_HALF_PHYS_START: usize = 0xffff_ffff_0000_0000;

#[no_mangle]
pub static KERNEL_STACK_START: usize = HIGH_HALF_PHYS_START + 0xffe0_0000;

const KERNEL_STACK_STARTING_SIZE: usize = 0x10000; // 64 KB
#[no_mangle]
pub static TRAMPOLINE_TRAP_FRAME: usize = HIGH_HALF_PHYS_START + 0xffe0_0000;
const TRAP_FRAME_SIZE: usize = 0x1000;

pub static mut LAST_L1_PTE: AtomicUsize = AtomicUsize::new(0);

pub const KERNEL_HEAP_START: usize = 0xffff_ffff_c000_0000;
pub const KERNEL_HEAP_SIZE: usize = 4 * memory::PAGE_SIZE;
static KERNEL_HEAP_END: AtomicUsize = AtomicUsize::new(KERNEL_HEAP_START + KERNEL_HEAP_SIZE);

static mut KERNEL_PAGE_MAPPER: MaybeUninit<page_mapper::PageMapper> = MaybeUninit::uninit();

pub fn init(memory: memory::Region) {
    init_page_mapper();

    identity_map_memory(memory);

    // Map hardware-specific MMIO regions based on CPU type
    match dtb::get_cpu_type() {
        dtb::CpuType::Qemu => {
            println!("Mapping QEMU UART at {:#x} - {:#x}", 0x1000_0000, 0x1000_0000 + memory::PAGE_SIZE);

            with_page_mapper(|mapper| {
                mapper.map_range(
                    0x1000_0000,
                    0x1000_0000,
                    0x1000_0000 + memory::PAGE_SIZE,
                    PageFlags::READ
                        | PageFlags::WRITE
                        | PageFlags::ACCESSED
                        | PageFlags::DIRTY,
                    page_mapper::PageSize::Size4K,
                );
            });

            println!("Mapping QEMU PLIC at {:#x} - {:#x}", 0x0c00_0000usize, 0x0c40_0000usize);

            with_page_mapper(|mapper| {
                mapper.map_range(
                    0x0c00_0000,
                    0x0c00_0000,
                    0x0c40_0000,  // Map 4MB for QEMU PLIC
                    PageFlags::READ
                        | PageFlags::WRITE
                        | PageFlags::ACCESSED
                        | PageFlags::DIRTY,
                    page_mapper::PageSize::Size4K,
                );
            });
        }

        dtb::CpuType::LicheeRVNano => {
            // T-Head C906 requires Strong Order flag for MMIO when MAEE=1
            let thead_flags = PageFlags::THEAD_SO;

            println!("Mapping NanoRV UART at {:#x} - {:#x}", 0x0414_0000, 0x0414_0000 + memory::PAGE_SIZE);

            with_page_mapper(|mapper| {
                mapper.map_range(
                    0x0414_0000,
                    0x0414_0000,
                    0x0414_0000 + memory::PAGE_SIZE,
                    PageFlags::READ
                        | PageFlags::WRITE
                        | PageFlags::ACCESSED
                        | PageFlags::DIRTY
                        | thead_flags,
                    page_mapper::PageSize::Size4K,
                );
            });

            println!("Mapping NanoRV PLIC at {:#x} - {:#x}", 0x7000_0000usize, 0x7040_0000usize);

            with_page_mapper(|mapper| {
                mapper.map_range(
                    0x7000_0000,
                    0x7000_0000,
                    0x7040_0000,  // Map 4MB for NanoRV PLIC
                    PageFlags::READ
                        | PageFlags::WRITE
                        | PageFlags::ACCESSED
                        | PageFlags::DIRTY
                        | thead_flags,
                    page_mapper::PageSize::Size4K,
                );
            });

            println!("Mapping NanoRV SD controller at {:#x} - {:#x}", 0x0431_0000usize, 0x0431_0000 + memory::PAGE_SIZE);

            with_page_mapper(|mapper| {
                mapper.map_range(
                    0x0431_0000,
                    0x0431_0000,
                    0x0431_0000 + memory::PAGE_SIZE,
                    PageFlags::READ
                        | PageFlags::WRITE
                        | PageFlags::ACCESSED
                        | PageFlags::DIRTY
                        | thead_flags,
                    page_mapper::PageSize::Size4K,
                );
            });
        }

        _ => {
            println!("WARNING: Unknown CPU type, no MMIO mapped");
        }
    }

    map_kernel_segments();

    map_kernel_stack();

    map_and_initialize_kernel_heap();

    map_last_l1_pte();

    switch_to_kernel_map();

    #[cfg(not(test))]
    unsafe {
        kernel_allocator::ALLOCATOR.init(KERNEL_HEAP_START, KERNEL_HEAP_SIZE);
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

    // T-Head C906 requires memory type flags for normal memory (for AMO instructions)
    let thead_mem_flags = if dtb::get_cpu_type() == dtb::CpuType::LicheeRVNano {
        PageFlags::THEAD_MEMORY
    } else {
        PageFlags::empty()
    };

    with_page_mapper(|mapper| {
        mapper.map_range(
            data_start,
            phys_data_start,
            data_end,
            PageFlags::READ | PageFlags::WRITE | PageFlags::ACCESSED | PageFlags::DIRTY | thead_mem_flags,
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

    // T-Head C906 requires memory type flags for normal memory (for AMO instructions)
    let thead_mem_flags = if dtb::get_cpu_type() == dtb::CpuType::LicheeRVNano {
        PageFlags::THEAD_MEMORY
    } else {
        PageFlags::empty()
    };

    with_page_mapper(|mapper| {
        mapper.map_range(
            bss_start,
            phys_bss_start,
            bss_end,
            PageFlags::READ | PageFlags::WRITE | PageFlags::ACCESSED | PageFlags::DIRTY | thead_mem_flags,
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

    // T-Head C906 requires memory type flags for normal memory
    let thead_mem_flags = if dtb::get_cpu_type() == dtb::CpuType::LicheeRVNano {
        PageFlags::THEAD_MEMORY
    } else {
        PageFlags::empty()
    };

    with_page_mapper(|mapper| {
        mapper.allocate_and_map_pages(
            kernel_stack_end,
            KERNEL_STACK_STARTING_SIZE,
            PageFlags::READ | PageFlags::WRITE | PageFlags::ACCESSED | PageFlags::DIRTY | thead_mem_flags,
        );
    });

}

fn map_and_initialize_kernel_heap() {
    println!(
        "Mapping kernel heap segment: virt: {:#x} - {:#x}",
        KERNEL_HEAP_START,
        KERNEL_HEAP_END.load(Ordering::SeqCst)
    );

    // T-Head C906 requires memory type flags for normal memory
    let thead_mem_flags = if dtb::get_cpu_type() == dtb::CpuType::LicheeRVNano {
        PageFlags::THEAD_MEMORY
    } else {
        PageFlags::empty()
    };

    with_page_mapper(|mapper| {
        mapper.allocate_and_map_pages(
            KERNEL_HEAP_START,
            KERNEL_HEAP_SIZE,
            PageFlags::READ | PageFlags::WRITE | PageFlags::ACCESSED | PageFlags::DIRTY | thead_mem_flags,
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


    // Map the trampoline trap frame segment
    println!(
        "Mapping trampoline trap frame segment: virt: {:#x} - {:#x}",
        TRAMPOLINE_TRAP_FRAME,
        TRAMPOLINE_TRAP_FRAME + TRAP_FRAME_SIZE,
    );

    // T-Head C906 requires memory type flags for normal memory
    let thead_mem_flags = if dtb::get_cpu_type() == dtb::CpuType::LicheeRVNano {
        PageFlags::THEAD_MEMORY
    } else {
        PageFlags::empty()
    };

    with_page_mapper(|mapper| {
        mapper.allocate_and_map_pages(
            TRAMPOLINE_TRAP_FRAME,
            TRAP_FRAME_SIZE,
            PageFlags::READ | PageFlags::WRITE | PageFlags::ACCESSED | PageFlags::DIRTY | thead_mem_flags,
        );
    });

    let last_l1_pte = kernel_page_mapper_ref().l1_page_table_from_phys(TRAMPOLINE_TRAP_FRAME);
    println!("mapped last l1 pte, pte is: {:#x}", last_l1_pte as usize);
    unsafe {
        LAST_L1_PTE.store(last_l1_pte as usize, Ordering::SeqCst);
    }
}

pub fn allocate_and_map_process_load_area_range(start: usize, size: usize, flags: PageFlags) {
    #[cfg(feature = "trace_process")]
    println!("Allocating and mapping process load range: virt: {:#x} - {:#x}", start, start + size);

    with_page_mapper(|mapper| {
        mapper.allocate_and_map_pages(
            start,
            size,
            flags,
        );
    });

    thead_flush_dcache();

    unsafe {
        core::arch::asm!("sfence.vma zero, zero", options(nostack, preserves_flags),);
    }
}

#[inline(always)]
pub fn switch_to_kernel_map() {
    let satp_value = kernel_page_mapper_ref().satp();

    println!("Switching to memory map with SATP value: {:#x}", satp_value);

    unsafe {
        core::arch::asm!(
            "csrw satp, {0}",
            in(reg) satp_value,
            options(nostack)
        );

        thead_flush_dcache();

        core::arch::asm!("sfence.vma zero, zero", options(nostack));
    }

    let tramp_trap_frame = TRAMPOLINE_TRAP_FRAME as *mut types::TrampolineTrapFrame;
    unsafe {
        (*tramp_trap_frame).kernel_satp = satp_value;
        (*tramp_trap_frame).is_lichee_rvnano =
            (dtb::get_cpu_type() == dtb::CpuType::LicheeRVNano) as usize;
    }

    println!("Switched to kernel map");
}

pub fn grow_kernel_heap(size: usize) -> Option<(usize, usize)> {
    #[cfg(feature = "trace_process")]
    println!("Growing kernel heap: {:#x}", size);
    let size = memory::align_up(size);

    let old_end = KERNEL_HEAP_END.load(Ordering::SeqCst);
    let new_end = old_end.checked_add(size)?;

    // T-Head C906 requires memory type flags for normal memory
    let thead_mem_flags = if dtb::get_cpu_type() == dtb::CpuType::LicheeRVNano {
        PageFlags::THEAD_MEMORY
    } else {
        PageFlags::empty()
    };

    with_page_mapper(|mapper| {
        mapper.allocate_and_map_pages(
            old_end,
            size,
            PageFlags::READ | PageFlags::WRITE | PageFlags::ACCESSED | PageFlags::DIRTY | thead_mem_flags,
        );
    });

    // Go back to using the function call approach
    thead_flush_dcache();

    unsafe {
        core::arch::asm!("sfence.vma zero, zero", options(nostack, preserves_flags),);
    }

    KERNEL_HEAP_END.store(new_end, Ordering::SeqCst);

    Some((old_end, size))
}

/// Flush T-Head C906 D-cache after page table modifications (same address space).
///
/// Use this after growing heap/stack within the same page table.
/// Cleans and invalidates D-cache so new PTEs are visible.
///
/// See notes/thead-c906-memory-guide.md for full cache instruction documentation.
#[inline]
pub fn thead_flush_dcache() {
    if dtb::get_cpu_type() == dtb::CpuType::LicheeRVNano {
        unsafe {
            core::arch::asm!(
                ".long 0x0030000b",   // th.dcache.ciall - clean and invalidate all D-cache
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

pub fn root_table() -> *const page_mapper::PageTable {
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
