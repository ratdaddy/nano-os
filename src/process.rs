use crate::cpu;
use crate::page_mapper;

#[repr(C)]
pub struct Context {
    pub registers: cpu::Registers,
    pub satp: usize,
    pub page_map: page_mapper::PageMapper,
    pub heap_begin: usize,
    pub heap_end: usize,
}

impl Context {
    pub fn new() -> Self {
        let registers = cpu::Registers::new();
        let page_map = page_mapper::PageMapper::new();
        let satp = page_map.satp();
        Context {
            registers,
            satp,
            page_map,
            heap_begin: 0,
            heap_end: 0,
        }
    }

    pub fn set_current(context: &mut Self) {
        unsafe { CURRENT_CONTEXT = context as *mut Context }
    }

    pub fn current<'a>() -> &'a mut Self {
        unsafe { &mut *CURRENT_CONTEXT }
    }
}

static mut CURRENT_CONTEXT: *mut Context = core::ptr::null_mut();
