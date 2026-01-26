#![allow(static_mut_refs)]

use crate::page_mapper;
use crate::process_memory_map;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::mem;
use core::ptr::addr_of_mut;

#[repr(C)]
pub struct Context {
    pub page_map: page_mapper::PageMapper,
    pub satp: usize,
    pub heap_begin: usize,
    pub heap_end: usize,
    pub mmap_next: usize,
    pub trap_frame: &'static mut types::ProcessTrapFrame,
    pub kernel_stack: [u8; 8192],
}

impl Context {
    pub fn new() -> Box<Self> {
        let mut boxed: Box<mem::MaybeUninit<Self>> = Box::new_uninit();
        let ptr = boxed.as_mut_ptr() as *mut Self;

        unsafe {
            let stack = addr_of_mut!((*ptr).kernel_stack);
            let stack_top = (*stack).as_ptr().add(8192) as usize;
            let tf_ptr = (stack_top - mem::size_of::<types::ProcessTrapFrame>()) as *mut types::ProcessTrapFrame;
            (*ptr).trap_frame = &mut *(tf_ptr as *mut _);

            (*ptr).page_map = page_mapper::PageMapper::new();
            (*ptr).satp = (*ptr).page_map.satp();
            (*ptr).heap_begin = 0;
            (*ptr).heap_end = 0;
            (*ptr).mmap_next = process_memory_map::PROCESS_MMAP_START;

            boxed.assume_init()
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
static mut PROCESS_TABLE: Option<Vec<Box<Context>>> = None;

pub fn init() {
    unsafe {
        PROCESS_TABLE = Some(Vec::new());
    }
}

pub fn create() -> &'static mut Context {
    let boxed = Context::new();

    unsafe {
        if PROCESS_TABLE.is_none() {
            PROCESS_TABLE = Some(Vec::new());
        }

        let table = PROCESS_TABLE.as_mut().unwrap();
        table.push(boxed);

        let last_ref: &mut Context = table.last_mut().unwrap();

        &mut *(last_ref as *mut Context)
    }
}
