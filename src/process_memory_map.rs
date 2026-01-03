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

pub const PROCESS_LOAD_AREA: usize = 0xffff_ffc0_0000_0000;
pub const PROCESS_LOAD_AREA_ENTRY: usize = 0x100;
pub const PROCESS_STACK_START: usize = 0xffe0_0000;
const PROCESS_STACK_STARTING_SIZE: usize = 0x4000;
pub const PROCESS_MMAP_START: usize = 0x1_0000_0000;

pub fn init_from_elf<R: io::Read + io::Seek>(elf_handle: &mut R, context: &mut process::Context) {
    let header = read_elf::read_elf64_header(elf_handle).unwrap();

    let (base_vaddr, heap_start) =
        load_elf(elf_handle, &header).expect("Failed to load ELF file");

    context.trap_frame.pc = header.e_entry as usize;
    println!("Setting heap address to {:#x} in context at {:#x}", heap_start, context as *const _ as usize);
    context.heap_begin = heap_start;
    context.heap_end = heap_start;

    let page_map = &mut context.page_map;

    unsafe {
        let process_load_area_entry = (*kernel_memory_map::root_table()).entries[PROCESS_LOAD_AREA_ENTRY];
        let process_load_area_page_table = process_load_area_entry.addr() as *mut page_mapper::PageTable;
        switch_pages_to_user(&mut *process_load_area_page_table);
        (*page_map.root_table).entries[0].set(process_load_area_page_table as usize, PageFlags::VALID);
        // TODO: unset kernel process load area table entry
    }

    // Map first page of process stack
    let process_stack_end = PROCESS_STACK_START - memory::PAGE_SIZE;
    println!(
        "Mapping first page of process stack: virt: {:#x} - {:#x}",
        process_stack_end, PROCESS_STACK_START
    );

    let first_stack_page = page_allocator::alloc().expect("Failed to allocate process first stack page");

    let mut sp = first_stack_page + memory::PAGE_SIZE;

    const AT_NULL: u64   = 0;
    const AT_PHDR: u64   = 3;
    const AT_PHENT: u64  = 4;
    const AT_PHNUM: u64  = 5;
    const AT_PAGESZ: u64 = 6;
    const AT_BASE: u64   = 7;
    const AT_SECURE: u64 = 23;

    // Insert auxiliary vector
    let phdr_addr = header.e_phoff + base_vaddr as u64;
    let phnum = header.e_phnum;
    let phent = header.e_phentsize;
    let pagesz = memory::PAGE_SIZE as u64;

    unsafe {
        let mut push_aux = |key: u64, value: u64| {
            sp -= 8;
            ptr::write(sp as *mut u64, value);
            sp -= 8;
            ptr::write(sp as *mut u64, key);
        };

        push_aux(AT_NULL, 0);
        push_aux(AT_SECURE, 0);
        push_aux(AT_BASE, 0);
        push_aux(AT_PAGESZ, pagesz);
        push_aux(AT_PHENT, phent.into());
        push_aux(AT_PHNUM, phnum.into());
        push_aux(AT_PHDR, phdr_addr);
    }

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
        context.trap_frame.registers.a1 = 0;

        // argc = 1
        sp -= mem::size_of::<u64>();
        ptr::write(sp as *mut u64, 1);
        context.trap_frame.registers.a0 = 1;
    }

    context.trap_frame.registers.sp = PROCESS_STACK_START - (first_stack_page + memory::PAGE_SIZE - sp);

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
        remaining_process_stack_end,
        PROCESS_STACK_STARTING_SIZE - memory::PAGE_SIZE,
        PageFlags::READ
            | PageFlags::WRITE
            | PageFlags::ACCESSED
            | PageFlags::DIRTY
            | PageFlags::USER,
    );

    // Map the last l1 page table
    let last_l1_pte = unsafe { kernel_memory_map::LAST_L1_PTE.load(Ordering::Relaxed) };

    page_map.set_l1_page_table_for_phys(
        kernel_memory_map::TRAMPOLINE_TRAP_FRAME,
        last_l1_pte as *mut page_mapper::PageTable,
    );
}

fn load_elf<R: io::Read + io::Seek>(
    elf_handle: &mut R,
    header: &read_elf::Elf64Header,
) -> Result<(usize, usize), &'static str> {
    let mut base_vaddr = usize::MAX;
    let mut heap_start = 0usize;

    let program_headers = read_elf::read_program_headers(elf_handle, &header).unwrap();

    for ph in &program_headers {
        if ph.p_type == read_elf::PT_LOAD {
            let offset = ph.p_offset as usize;
            let virt_addr = ph.p_vaddr as usize;
            let phys_addr = ph.p_paddr as usize;
            let mem_size = ph.p_memsz as usize;
            let file_size = ph.p_filesz as usize;

            if offset == 0 {
                base_vaddr = virt_addr;
            }

            let virt_addr_start = virt_addr + PROCESS_LOAD_AREA;
            let virt_addr_end = memory::align_up(virt_addr + mem_size);

            let virt_addr_load_start = memory::align_down(virt_addr_start);
            let virt_addr_load_end = virt_addr_end + PROCESS_LOAD_AREA;
            if virt_addr_end > heap_start {
                heap_start = virt_addr_end;
            }

            println!(
                "Mapping ELF segment: virt: {:#x} - {:#x} to phys: {:#x}",
                virt_addr_load_start,
                virt_addr_load_end,
                phys_addr
            );

            kernel_memory_map::allocate_and_map_process_load_area_range(
                virt_addr_load_start,
                virt_addr_load_end - virt_addr_load_start,
                PageFlags::READ | PageFlags::WRITE | PageFlags::EXECUTE | PageFlags::ACCESSED | PageFlags::DIRTY,
            );

            elf_handle.seek(io::SeekFrom::Start(offset))
                .map_err(|_| "Failed to seek to program header offset")?;

            println!("Loading from {:#x}", virt_addr_start);
            let buffer = unsafe { slice::from_raw_parts_mut(virt_addr_start as *mut u8, file_size) };
            elf_handle.read_exact(buffer)
                .map_err(|_| "Failed to read ELF segment data")?;

            println!("Zeroing out {:#x} remaining bytes in ELF segment: virt: {:#x}", mem_size - file_size, virt_addr_start + file_size);
            unsafe {
                core::ptr::write_bytes((virt_addr_start + file_size) as *mut u8, 0, mem_size - file_size);
            }
        }
    }

    Ok((base_vaddr, heap_start))
}

fn switch_pages_to_user(process_load_area_entry: &mut page_mapper::PageTable) {
    for l1_entry in process_load_area_entry.entries.iter_mut() {
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
