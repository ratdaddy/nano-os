use core::ptr::null_mut;

#[cfg(not(test))]
use crate::kernel_memory_map;
#[cfg(test)]
use crate::memory;

pub const BLOCK_HEADER_SIZE: usize = size_of::<BlockHeader>();

const NULL_OFFSET: u32 = 0xffff_fffe;
const OFFSET_LOWER_BITS_MASK: u32 = !NULL_OFFSET;

#[repr(C)]
#[derive(Debug)]
pub struct BlockHeader {
    _size: u32,
    _prev: u32,
    _free_next: u32,
    _free_prev: u32,
}

impl BlockHeader {
    pub fn new(size: usize, used: bool) -> Self {
        BlockHeader {
            _size: size as u32,
            _prev: NULL_OFFSET,
            _free_next: NULL_OFFSET,
            _free_prev: NULL_OFFSET | if used { 1 } else { 0 },
        }
    }

    #[inline]
    pub fn alloc_area_start(&self) -> *mut u8 {
        unsafe { (self as *const _ as *mut u8).add(BLOCK_HEADER_SIZE) }
    }

    #[inline]
    pub fn next(&self) -> *mut BlockHeader {
        let this = self as *const BlockHeader;
        (this as usize + self._size as usize + BLOCK_HEADER_SIZE) as *mut BlockHeader
    }

    #[inline]
    pub fn prev(&self) -> *mut BlockHeader {
        from_offset(self._prev)
    }

    #[inline]
    pub fn set_prev(&mut self, prev: *mut BlockHeader) {
        self._prev = to_offset(prev);
    }

    #[inline]
    pub fn free_next(&self) -> *mut BlockHeader {
        from_offset(self._free_next)
    }

    #[inline]
    pub fn set_free_next(&mut self, next: *mut BlockHeader) {
        self._free_next = to_offset(next);
    }

    #[inline]
    pub fn free_prev(&self) -> *mut BlockHeader {
        let free_prev_offset = self._free_prev & NULL_OFFSET;

        if free_prev_offset == NULL_OFFSET {
            null_mut()
        } else {
            (heap_start() + free_prev_offset as usize) as *mut BlockHeader
        }
    }

    #[inline]
    pub fn set_free_prev(&mut self, prev: *mut BlockHeader) {
        self._free_prev &= OFFSET_LOWER_BITS_MASK;

        if prev.is_null() {
            self._free_prev |= NULL_OFFSET;
        } else {
            self._free_prev |= (prev as *const _ as usize - heap_start()) as u32
        }
    }

    #[inline]
    pub fn size(&self) -> usize {
        self._size as usize
    }

    #[inline]
    pub fn set_size(&mut self, size: usize) {
        self._size = size as u32;
    }

    #[inline]
    pub fn add_size(&mut self, size: usize) {
        self._size += size as u32;
    }

    #[inline]
    pub fn is_used(&self) -> bool {
        (self._free_prev & 1) != 0
    }

    #[inline]
    pub fn set_used(&mut self) {
        self._free_prev |= 1;
    }

    #[inline]
    pub fn is_free(&self) -> bool {
        (self._free_prev & 1) == 0
    }

    #[inline]
    pub fn set_free(&mut self) {
        self._free_prev &= !1;
    }

    #[inline]
    pub fn end_ptr(&self) -> usize {
        self as *const _ as usize + BLOCK_HEADER_SIZE + self._size as usize
    }
}

#[inline]
fn from_offset(offset: u32) -> *mut BlockHeader {
    if offset == NULL_OFFSET {
        null_mut()
    } else {
        (heap_start() + offset as usize) as *mut BlockHeader
    }
}

#[inline]
fn to_offset(pointer: *mut BlockHeader) -> u32 {
    if pointer.is_null() {
        NULL_OFFSET
    } else {
        (pointer as *const _ as usize - heap_start()) as u32
    }
}

#[cfg(test)]
pub const TEST_HEAP_SIZE: usize = memory::PAGE_SIZE * 2;

#[cfg(test)]
#[repr(align(4096))]
pub struct AlignedHeap(pub [u8; TEST_HEAP_SIZE]);

#[cfg(test)]
pub static mut TEST_HEAP: AlignedHeap = AlignedHeap([0; TEST_HEAP_SIZE]);

#[cfg(test)]
#[inline]
fn heap_start() -> usize {
    unsafe { TEST_HEAP.0.as_ptr() as usize }
}

#[cfg(not(test))]
#[inline]
fn heap_start() -> usize {
    kernel_memory_map::KERNEL_HEAP_START
}
