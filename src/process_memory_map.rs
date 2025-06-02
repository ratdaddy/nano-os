use core::{ mem, ptr };
use core::slice;
use core::sync::atomic::Ordering;

use crate::io;
use crate::kernel_memory_map;
use crate::memory;
use crate::page_allocator;
use crate::page_mapper::{self, PageFlags};
use crate::process;
use crate::read_elf;

pub const PROCESS_STACK_START: usize = 0xffe0_0000;
const PROCESS_STACK_STARTING_SIZE: usize = 0x4000;

pub fn init_from_elf<R: io::Read + io::Seek>(elf_handle: &mut R, context: &mut process::Context) {
    let page_map = &mut context.page_map;

    context.registers.pc = load_elf(elf_handle).expect("Failed to load ELF file");

    unsafe {
        let zeropage_l1_entry = (*kernel_memory_map::root_table()).entries[0];
        let zeropage_l1_page_table = zeropage_l1_entry.addr() as *mut page_mapper::PageTable;
        switch_pages_to_user(&mut *zeropage_l1_page_table);
        (*page_map.root_table).entries[0].set(zeropage_l1_page_table as usize, PageFlags::VALID);
    }

    // Map first page of process stack
    let process_stack_end = PROCESS_STACK_START - memory::PAGE_SIZE;
    println!(
        "Mapping first page of process stack: virt: {:#x} - {:#x}",
        process_stack_end, PROCESS_STACK_START
    );

    let first_stack_page = page_allocator::alloc().expect("Failed to allocate process first stack page");

    let mut sp = first_stack_page + memory::PAGE_SIZE;

    unsafe {
        // envp NULL
        sp -= mem::size_of::<u64>();
        ptr::write(sp as *mut u64, 0);

        // argv[1] = NULL
        sp -= mem::size_of::<u64>();
        ptr::write(sp as *mut u64, 0);

        // dummy argv[0] pointer (could be pointer to some string, here just zero)
        sp -= mem::size_of::<u64>();
        ptr::write(sp as *mut u64, 0);
        context.registers.a1 = 0;

        // argc = 1
        sp -= mem::size_of::<u64>();
        ptr::write(sp as *mut u64, 1);
        context.registers.a0 = 1;
    }

    context.registers.sp = PROCESS_STACK_START - (first_stack_page + memory::PAGE_SIZE - sp);

    page_map.map_range(
        process_stack_end,
        first_stack_page,
        PROCESS_STACK_START,
        PageFlags::READ
            | PageFlags::WRITE
            | PageFlags::ACCESSED
            | PageFlags::DIRTY
            | PageFlags::USER,
        page_mapper::PageSize::Size4K,
    );

    // Map the remaining process stack segment
    let remaining_process_stack_end = process_stack_end - (PROCESS_STACK_STARTING_SIZE - memory::PAGE_SIZE);
    println!(
        "Mapping remaining process stack segment: virt: {:#x} - {:#x}",
        remaining_process_stack_end, process_stack_end
    );

    page_map.allocate_and_map_pages(
        process_stack_end,
        PROCESS_STACK_STARTING_SIZE,
        PageFlags::READ
            | PageFlags::WRITE
            | PageFlags::ACCESSED
            | PageFlags::DIRTY
            | PageFlags::USER,
    );

    // Map the last l1 page table
    let last_l1_pte = unsafe { kernel_memory_map::LAST_L1_PTE.load(Ordering::Relaxed) };

    page_map.set_l1_page_table_for_phys(
        kernel_memory_map::TRAP_FRAME,
        last_l1_pte as *mut page_mapper::PageTable,
    );
}

fn load_elf<R: io::Read + io::Seek>(elf_handle: &mut R) -> Result<usize, &'static str> {
    let header = read_elf::read_elf64_header(elf_handle).unwrap();

    let entry_point = header.e_entry as usize;

    let program_headers = read_elf::read_program_headers(elf_handle, &header).unwrap();

    for ph in &program_headers {
        if ph.p_type == read_elf::PT_LOAD {
            let offset = ph.p_offset as usize;
            let virt_addr = ph.p_vaddr as usize;
            let phys_addr = ph.p_paddr as usize;
            let size = ph.p_memsz as usize;

            let virt_page_addr_start = memory::align_down(virt_addr);
            let virt_page_addr_end = memory::align_up(virt_addr + size);

            println!(
                "Mapping ELF segment: virt: {:#x} - {:#x} to phys: {:#x}",
                virt_addr,
                virt_addr + size,
                phys_addr
            );

            kernel_memory_map::allocate_and_map_zeropage_range(
                virt_page_addr_start,
                virt_page_addr_end - virt_page_addr_start,
                PageFlags::READ | PageFlags::WRITE | PageFlags::EXECUTE | PageFlags::ACCESSED | PageFlags::DIRTY,
            );

            elf_handle.seek(io::SeekFrom::Start(offset))
                .map_err(|_| "Failed to seek to program header offset")?;

            let buffer = unsafe { slice::from_raw_parts_mut(virt_addr as *mut u8, size) };
            elf_handle.read_exact(buffer)
                .map_err(|_| "Failed to read ELF segment data")?;
        }
    }

    Ok(entry_point)
}

fn switch_pages_to_user(zeropage_l1_entry: &mut page_mapper::PageTable) {
    for l1_entry in zeropage_l1_entry.entries.iter_mut() {
        if l1_entry.is_valid() {
            let l2_table = l1_entry.addr() as *mut page_mapper::PageTable;
            unsafe {
                for l2_entry in (*l2_table).entries.iter_mut() {
                    if l2_entry.is_valid() {
                        l2_entry.set_user_flag();
                    }
                }
            }
        }
    }
}
