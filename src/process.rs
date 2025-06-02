use crate::cpu;
use crate::page_mapper;

#[repr(C)]
pub struct Context {
    pub registers: cpu::Registers,
    pub satp: usize,
    pub page_map: page_mapper::PageMapper,
}

impl Context {
    pub fn new() -> Self {
        let registers = cpu::Registers::new();
        let page_map = page_mapper::PageMapper::new();
        let satp = page_map.satp();
        Context { registers, satp, page_map }
    }
}
