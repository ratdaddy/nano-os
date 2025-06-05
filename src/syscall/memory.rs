use crate::trap::TrapFrame;
use crate::{memory, page_mapper::PageFlags, process};

pub fn mmap(tf: &mut TrapFrame) {
    tf.registers.a0 = usize::MAX - 37 + 1;
}

pub fn brk(tf: &mut TrapFrame) {
    let size = tf.registers.a0;
    let ctx = process::Context::current();

    if size != 0 {
        let size = memory::align_up(size);

        ctx.page_map.allocate_and_map_pages(
            ctx.heap_end,
            size,
            PageFlags::READ
                | PageFlags::WRITE
                | PageFlags::ACCESSED
                | PageFlags::DIRTY
                | PageFlags::USER,
        );

        ctx.heap_end += size;
    }

    tf.registers.a0 = ctx.heap_end;
}
