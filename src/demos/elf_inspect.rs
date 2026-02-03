//! Demo: Inspect ELF file headers from initramfs.

use crate::initramfs;
use crate::read_elf;

pub fn inspect_elf() {
    let path = "/prog_example";

    println!("Inspecting ELF file: {}", path);

    let mut handle = match initramfs::ifs_open(path) {
        Ok(h) => h,
        Err(e) => {
            println!("Failed to open {}: {}", path, e);
            return;
        }
    };

    let header = match read_elf::read_elf64_header(&mut handle) {
        Ok(h) => h,
        Err(e) => {
            println!("Failed to read ELF header: {:?}", e);
            return;
        }
    };

    println!();
    println!("=== ELF Header ===");
    println!("Entry point:     {:#x}", header.e_entry);
    println!("PH offset:       {:#x}", header.e_phoff);
    println!("PH entry size:   {}", header.e_phentsize);
    println!("PH count:        {}", header.e_phnum);

    let program_headers = match read_elf::read_program_headers(&mut handle, &header) {
        Ok(phs) => phs,
        Err(e) => {
            println!("Failed to read program headers: {:?}", e);
            return;
        }
    };

    println!();
    println!("=== Program Headers ===");
    for (i, ph) in program_headers.iter().enumerate() {
        let type_str = match ph.p_type {
            read_elf::PT_NULL => "NULL",
            read_elf::PT_LOAD => "LOAD",
            read_elf::PT_DYNAMIC => "DYNAMIC",
            read_elf::PT_INTERP => "INTERP",
            read_elf::PT_NOTE => "NOTE",
            read_elf::PT_PHDR => "PHDR",
            _ => "OTHER",
        };

        println!("[{}] Type: {} ({:#x})", i, type_str, ph.p_type);
        println!("    Offset:    {:#x}", ph.p_offset);
        println!("    VirtAddr:  {:#x} - {:#x}", ph.p_vaddr, ph.p_vaddr + ph.p_memsz);
        println!("    PhysAddr:  {:#x}", ph.p_paddr);
        println!("    FileSize:  {:#x}", ph.p_filesz);
        println!("    MemSize:   {:#x}", ph.p_memsz);
        println!("    Flags:     {:#x}", ph.p_flags);
        println!();
    }
}
