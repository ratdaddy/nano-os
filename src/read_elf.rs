#![allow(dead_code)]

use core::mem::size_of;

use crate::file::{self, File};
use crate::vfs;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Header {
    pub e_ident: [u8; 16], // Magic + class + endianness + etc
    pub e_type: u16,       // Object file type (ET_EXEC, etc)
    pub e_machine: u16,    // Architecture (EM_RISCV, etc)
    pub e_version: u32,    // Must be 1
    pub e_entry: u64,      // Entry point virtual address
    pub e_phoff: u64,      // Program header table offset
    pub e_shoff: u64,      // Section header table offset (not needed for loading)
    pub e_flags: u32,
    pub e_ehsize: u16,     // ELF header size
    pub e_phentsize: u16,  // Program header entry size
    pub e_phnum: u16,      // Number of program headers
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64ProgramHeader {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

pub const PT_NULL:        u32 = 0;          // Unused entry
pub const PT_LOAD:        u32 = 1;          // Loadable segment
pub const PT_DYNAMIC:     u32 = 2;          // Dynamic linking information
pub const PT_INTERP:      u32 = 3;          // Path to interpreter
pub const PT_NOTE:        u32 = 4;          // Auxiliary information
pub const PT_SHLIB:       u32 = 5;          // Reserved, unspecified
pub const PT_PHDR:        u32 = 6;          // Program header table itself
pub const PT_TLS:         u32 = 7;          // Thread-local storage template

// GNU extensions (typically used in statically linked binaries)
pub const PT_GNU_EH_FRAME: u32 = 0x6474e550; // Exception handling info
pub const PT_GNU_STACK:    u32 = 0x6474e551; // Stack flags (e.g., executable)
pub const PT_GNU_RELRO:    u32 = 0x6474e552; // Read-only after relocation

pub fn read_elf64_header(reader: &mut File) -> Result<Elf64Header, file::Error> {
    let mut buf = [0u8; size_of::<Elf64Header>()];
    vfs::vfs_read_exact(reader, &mut buf)?;

    let header = unsafe { core::ptr::read(buf.as_ptr() as *const Elf64Header) };
    Ok(header)
}

pub fn read_program_headers(reader: &mut File, header: &Elf64Header) -> Result<alloc::vec::Vec<Elf64ProgramHeader>, file::Error> {
    vfs::vfs_seek(reader, file::SeekFrom::Start(header.e_phoff as usize))?;

    let mut result = alloc::vec::Vec::with_capacity(header.e_phnum as usize);

    for _ in 0..header.e_phnum {
        let phdr = read_program_header(reader)?;
        result.push(phdr);
    }

    Ok(result)
}

pub fn read_program_header(reader: &mut File) -> Result<Elf64ProgramHeader, file::Error> {
    let mut buf = [0u8; core::mem::size_of::<Elf64ProgramHeader>()];
    vfs::vfs_read_exact(reader, &mut buf)?;
    let phdr = unsafe { core::ptr::read(buf.as_ptr() as *const Elf64ProgramHeader) };
    Ok(phdr)
}
