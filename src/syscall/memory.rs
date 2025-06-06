use crate::trap::TrapFrame;
use crate::{memory, page_mapper::PageFlags, process};

pub fn mmap(tf: &mut TrapFrame) {
    let addr_hint = tf.registers.a0;
    let len = tf.registers.a1;
    let prot = tf.registers.a2;
    let flags = tf.registers.a3;

    println!(
        "[mmap] addr: {:#x}, length: {:#x}, prot: {:#x}, flags: {:#x}, fd: {}, offset: {:#x}",
        addr_hint,
        len,
        prot,
        flags,
        tf.registers.a4, // fd
        tf.registers.a5, // offset
    );

    // Only anonymous, private mappings with read/write permissions are
    // implemented.  Require addr = 0 so the kernel chooses the address.
    const EXPECTED_PROT: usize = 0x3; // PROT_READ | PROT_WRITE
    const EXPECTED_FLAGS: usize = 0x22; // MAP_PRIVATE | MAP_ANONYMOUS

    if addr_hint != 0 || len == 0 || prot != EXPECTED_PROT || flags != EXPECTED_FLAGS {
        tf.registers.a0 = usize::MAX - 37 + 1;
        //tf.registers.a0 = (-22i64) as usize;
        return;
    }

    let ctx = process::Context::current();
    let size = memory::align_up(len);

    let virt_addr = ctx.mmap_next;

    ctx.page_map.allocate_and_map_pages(
        virt_addr,
        size,
        PageFlags::READ
            | PageFlags::WRITE
            | PageFlags::ACCESSED
            | PageFlags::DIRTY
            | PageFlags::USER,
    );

    ctx.mmap_next += size;

    tf.registers.a0 = virt_addr;
}

pub fn brk(tf: &mut TrapFrame) {
    let size = tf.registers.a0;
    let ctx = process::Context::current();

    if size != 0 {
        let size = memory::align_up(size) - ctx.heap_end;

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

    println!("[brk] size: {:#x}, heap_end: {:#x}", size, ctx.heap_end);

    tf.registers.a0 = ctx.heap_end;
}
