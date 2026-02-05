#![allow(static_mut_refs)]

use crate::page_mapper;
use crate::process_memory_map;
use alloc::boxed::Box;

/// Process context - contains page tables and memory layout info.
/// The ProcessTrapFrame lives on the owning Thread's stack.
#[repr(C)]
pub struct Context {
    pub page_map: page_mapper::PageMapper,
    pub satp: usize,
    pub heap_begin: usize,
    pub heap_end: usize,
    pub mmap_next: usize,
    pub trap_frame: &'static mut types::ProcessTrapFrame,
}

// Safety: Context is only accessed by its owning thread. The raw pointers
// inside PageMapper point to page tables that are stable for the process lifetime.
unsafe impl Send for Context {}

impl Context {
    /// Create a new process context with the given trap frame location.
    /// The trap_frame should point to memory on the owning Thread's stack.
    pub fn new(trap_frame: &'static mut types::ProcessTrapFrame) -> Box<Self> {
        let page_map = page_mapper::PageMapper::new();
        let satp = page_map.satp();

        Box::new(Context {
            page_map,
            satp,
            heap_begin: 0,
            heap_end: 0,
            mmap_next: process_memory_map::PROCESS_MMAP_START,
            trap_frame,
        })
    }

    pub fn set_current(context: &mut Self) {
        unsafe { CURRENT_CONTEXT = context as *mut Context }
    }

    pub fn current<'a>() -> &'a mut Self {
        unsafe { &mut *CURRENT_CONTEXT }
    }
}

static mut CURRENT_CONTEXT: *mut Context = core::ptr::null_mut();
