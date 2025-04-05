// src/dtb.rs

use core::ptr;

extern "C" {
    static _start: u8;
}

const FDT_MAGIC: u32 = 0xd00dfeed;
const FDT_BEGIN_NODE: u32 = 1;
const FDT_END_NODE: u32 = 2;
const FDT_PROP: u32 = 3;
const FDT_NOP: u32 = 4;
const FDT_END: u32 = 9;

#[derive(Debug, Copy, Clone)]
pub struct MemoryRegion {
    pub base: u64,
    pub size: u64,
}

#[derive(Debug, Clone, Copy)]
pub enum DtbToken {
    BeginNode,
    EndNode,
    Prop,
    Nop,
    End,
    Unknown
}

pub struct DtbContext {
    pub struct_ptr: *const u8,
    pub strings_ptr: *const u8,
}

unsafe fn read_be32(ptr: *const u8) -> u32 {
    u32::from_be(ptr::read_unaligned(ptr as *const u32))
}

unsafe fn read_be64(ptr: *const u8) -> u64 {
    u64::from_be(ptr::read_unaligned(ptr as *const u64))
}

unsafe fn read_strz(ptr: *const u8) -> (&'static str, *const u8) {
    let mut end = ptr;
    while *end != 0 {
        end = end.add(1);
    }
    let len = end.offset_from(ptr) as usize;
    let s = core::str::from_utf8_unchecked(core::slice::from_raw_parts(ptr, len));
    let aligned = (((end.add(1) as usize) + 3) & !3) as *const u8;
    (s, aligned)
}

pub unsafe fn parse_dtb(dtb: *const u8) -> DtbContext {
    assert_eq!(read_be32(dtb), FDT_MAGIC, "Invalid DTB magic");
    let struct_ptr = dtb.add(read_be32(dtb.add(8)) as usize);
    let strings_ptr = dtb.add(read_be32(dtb.add(12)) as usize);
    DtbContext { struct_ptr, strings_ptr }
}

pub unsafe fn traverse_dtb<F: FnMut(DtbToken, usize, Option<&str>, Option<(&str, *const u8, usize)>)>(ctx: &DtbContext, mut f: F) {
    let mut p = ctx.struct_ptr;
    let mut depth: usize = 0;
    loop {
        let token_val = read_be32(p);
        p = p.add(4);
        let token = match token_val {
            FDT_BEGIN_NODE => {
                let (name, next) = read_strz(p);
                p = next;
                f(DtbToken::BeginNode, depth, Some(name), None);
                depth += 1;
                continue;
            }
            FDT_END_NODE => {
                depth = depth.saturating_sub(1);
                DtbToken::EndNode
            }
            FDT_PROP => {
                let len = read_be32(p) as usize;
                let nameoff = read_be32(p.add(4)) as usize;
                let data = p.add(8);
                p = data.add((len + 3) & !3);
                let name_ptr = ctx.strings_ptr.add(nameoff);
                let mut end = name_ptr;
                while *end != 0 {
                    end = end.add(1);
                }
                let name_len = end.offset_from(name_ptr) as usize;
                let name = core::str::from_utf8_unchecked(core::slice::from_raw_parts(name_ptr, name_len));
                f(DtbToken::Prop, depth, Some(""), Some((name, data, len)));
                continue;
            }
            FDT_NOP => DtbToken::Nop,
            FDT_END => DtbToken::End,
            _other => DtbToken::Unknown,
        };
        f(token, 0, None, None);
        if let DtbToken::End = token {
            break;
        }
    }
}

pub unsafe fn check_memory_layout(dtb: *const u8) {
    let ctx = parse_dtb(dtb);
    let mut addr_cells = 0;
    let mut size_cells = 0;
    let mut max_reserved_end: u64 = 0;
    let kernel_start = &_start as *const u8 as usize as u64;

    let mut in_reserved_memory = false;

    traverse_dtb(&ctx, |token, depth, name_opt, prop_opt| {
        if let DtbToken::BeginNode = token {
            if let Some(name) = name_opt {
                in_reserved_memory = name.starts_with("reserved-memory") || name.starts_with("mmode_resv");
            }
        } else if let DtbToken::Prop = token {
            if let Some((name, data, len)) = prop_opt {
                if depth == 1 {
                    if name == "#address-cells" {
                        addr_cells = read_be32(data);
                    } else if name == "#size-cells" {
                        size_cells = read_be32(data);
                    }
                }

                if in_reserved_memory && name == "reg" && len >= 16 {
                    let base = read_be64(data);
                    let size = read_be64(data.add(8));
                    let end = base + size;
                    if end > max_reserved_end {
                        max_reserved_end = end;
                    }
                }
            }
        }
    });

    assert_eq!(addr_cells, 2, "DTB must have #address-cells = 2");
    assert_eq!(size_cells, 2, "DTB must have #size-cells = 2");
    assert!(kernel_start >= max_reserved_end, "Kernel start address overlaps reserved memory!");
}

pub unsafe fn get_usable_memory(dtb: *const u8) -> Option<MemoryRegion> {
    let ctx = parse_dtb(dtb);
    let mut found = None;
    let mut in_memory_node = false;

    traverse_dtb(&ctx, |token, _depth, name_opt, prop_opt| {
        match token {
            DtbToken::BeginNode => {
                if let Some(name) = name_opt {
                    in_memory_node = name.starts_with("memory@");
                }
            }
            DtbToken::EndNode => in_memory_node = false,
            DtbToken::Prop if in_memory_node => {
                if let Some((name, data, len)) = prop_opt {
                    if name == "reg" && len >= 16 {
                        let base = read_be64(data);
                        let size = read_be64(data.add(8));
                        found = Some(MemoryRegion { base, size });
                    }
                }
            }
            _ => {}
        }
    });

    found
}

#[allow(dead_code)]
pub unsafe fn print_dtb(dtb: *const u8) {
    let ctx = parse_dtb(dtb);
    let mut indent: usize = 0;

    traverse_dtb(&ctx, |token, _depth, name_opt, prop_opt| {
        match token {
            DtbToken::BeginNode => {
                if let Some(name) = name_opt {
                    for _ in 0..indent {
                        print!("  ");
                    }
                    println!("{}", name);
                    indent += 1;
                }
            }
            DtbToken::EndNode => {
                indent = indent.saturating_sub(1);
            }
            DtbToken::Prop => {
                if let Some((name, data, len)) = prop_opt {
                    for _ in 0..indent {
                        print!("  ");
                    }
                    if len == 4 {
                        let val = read_be32(data);
                        println!("{} = 0x{:08x}", name, val);
                    } else if len % 8 == 0 {
                        print!("{} =", name);
                        for i in 0..(len / 8) {
                            let val = read_be64(data.add(i * 8));
                            print!(" 0x{:016x}", val);
                        }
                        println!();
                    } else if len > 0 && *data.add(len - 1) == 0 {
                        let s = core::str::from_utf8_unchecked(core::slice::from_raw_parts(data, len - 1));
                        println!("{} = \"{}\"", name, s);
                    } else {
                        print!("{} =", name);
                        for i in 0..len {
                            print!(" {:02x}", *data.add(i));
                        }
                        println!();
                    }
                }
            }
            _ => {}
        }
    });
}
