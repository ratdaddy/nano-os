use core::mem::size_of;

use crate::io;

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

pub fn read_elf64_header<R: io::Read>(reader: &mut R) -> Result<Elf64Header, io::Error> {
    let mut buf = [0u8; size_of::<Elf64Header>()];
    reader.read_exact(&mut buf)?;

    let header = unsafe { core::ptr::read(buf.as_ptr() as *const Elf64Header) };
    Ok(header)
}

pub fn read_program_headers<R>(reader: &mut R, header: &Elf64Header) -> Result<alloc::vec::Vec<Elf64ProgramHeader>, io::Error>
where
    R: io::Read + io::Seek,
{
    reader.seek(io::SeekFrom::Start(header.e_phoff as usize))?;

    let mut result = alloc::vec::Vec::with_capacity(header.e_phnum as usize);

    for _ in 0..header.e_phnum {
        let phdr = read_program_header(reader)?;
        result.push(phdr);
    }

    Ok(result)
}

pub fn read_program_header<R: io::Read>(reader: &mut R) -> Result<Elf64ProgramHeader, io::Error> {
    let mut buf = [0u8; core::mem::size_of::<Elf64ProgramHeader>()];
    reader.read_exact(&mut buf)?;
    let phdr = unsafe { core::ptr::read(buf.as_ptr() as *const Elf64ProgramHeader) };
    Ok(phdr)
}
