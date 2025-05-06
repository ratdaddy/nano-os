use core::ptr;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use crate::memory;

const FDT_MAGIC: u32 = 0xd00d_feed;
const FDT_BEGIN_NODE: u32 = 1;
const FDT_END_NODE: u32 = 2;
const FDT_PROP: u32 = 3;
const FDT_NOP: u32 = 4;
const FDT_END: u32 = 9;

#[derive(Debug, Clone, Copy)]
pub enum DtbToken {
    BeginNode,
    EndNode,
    Prop,
    Nop,
    End,
    Unknown,
}

pub struct DtbContext {
    pub total_size: usize,
    pub struct_ptr: *const u8,
    pub strings_ptr: *const u8,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CpuType {
    Unknown = 0,
    Qemu = 1,
    LicheeRVNano = 2,
}

static CPU_TYPE: AtomicU8 = AtomicU8::new(CpuType::Unknown as u8);
pub static INITRD_START: AtomicUsize = AtomicUsize::new(0);
static INITRD_END: AtomicUsize = AtomicUsize::new(0);

pub unsafe fn parse_dtb(dtb: *const u8) -> DtbContext {
    assert_eq!(read_be32(dtb), FDT_MAGIC, "Invalid DTB magic");
    let total_size = read_be32(dtb.add(4)) as usize;
    let struct_ptr = dtb.add(read_be32(dtb.add(8)) as usize);
    let strings_ptr = dtb.add(read_be32(dtb.add(12)) as usize);
    DtbContext { total_size, struct_ptr, strings_ptr }
}

pub unsafe fn traverse_dtb<
    F: FnMut(DtbToken, usize, Option<&str>, Option<(&str, *const u8, usize)>),
>(
    ctx: &DtbContext,
    mut f: F,
) {
    let mut p = ctx.struct_ptr;
    let mut depth: usize = 0;
    loop {
        let token_val = read_be32(p);
        p = p.add(4);
        match token_val {
            FDT_BEGIN_NODE => {
                let (name, next) = read_strz(p);
                p = next;
                f(DtbToken::BeginNode, depth, Some(name), None);
                depth += 1;
            }
            FDT_END_NODE => {
                depth = depth.saturating_sub(1);
                f(DtbToken::EndNode, depth, None, None);
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
                let name =
                    core::str::from_utf8_unchecked(core::slice::from_raw_parts(name_ptr, name_len));
                f(DtbToken::Prop, depth, Some(""), Some((name, data, len)));
            }
            FDT_NOP => {
                f(DtbToken::Nop, depth, None, None);
            }
            FDT_END => {
                f(DtbToken::End, depth, None, None);
                break;
            }
            _ => {
                f(DtbToken::Unknown, depth, None, None);
            }
        }
    }
}

pub unsafe fn collect_memory_map<const N: usize>(
    dtb: *const u8,
    reserved: &mut heapless::Vec<memory::Region, N>,
) -> Option<memory::Region> {
    let ctx = parse_dtb(dtb);
    let mut memory: Option<memory::Region> = None;
    let mut memory_active = false;
    let mut reserved_active = false;
    let mut chosen_active = false;
    let mut initrd_start = None;
    let mut initrd_end = None;

    traverse_dtb(&ctx, |token, depth, name_opt, prop_opt| {
        match token {
            // Only look for these top‑level nodes at depth == 1
            DtbToken::BeginNode if depth == 1 => {
                if let Some(name) = name_opt {
                    match name {
                        "reserved-memory" => reserved_active = true,
                        "chosen" => chosen_active = true,
                        _ if name.starts_with("memory@") => memory_active = true,
                        _ => {}
                    }
                }
            }
            // Reset when leaving any depth‑1 node
            DtbToken::EndNode if depth == 1 => {
                memory_active = false;
                reserved_active = false;
            }
            DtbToken::Prop => {
                if let Some((prop_name, data, len)) = prop_opt {
                    // Usable memory: capture both start & size
                    if memory_active && prop_name == "reg" && len >= 16 {
                        let start = read_be64(data) as usize;
                        let size = read_be64(data.add(8)) as usize;
                        memory = Some(memory::Region { start, end: start + size });
                    }
                    // Reserved regions: reg or alloc-ranges
                    else if reserved_active
                        && (prop_name == "reg" || prop_name == "alloc-ranges")
                        && len >= 16
                    {
                        let start = read_be64(data) as usize;
                        let size = read_be64(data.add(8)) as usize;
                        let aligned_start = memory::align_down(start);
                        let aligned_end = memory::align_up(start + size);

                        assert!(
                            aligned_start <= aligned_end,
                            "Invalid reserved memory region: start > end"
                        );
                        let _ = reserved
                            .push(memory::Region { start: aligned_start, end: aligned_end });
                    }
                    // Chosen region: capture the initrd start/end
                    else if chosen_active {
                        if prop_name == "linux,initrd-start" {
                            initrd_start = Some(read_be64(data) as usize);
                        } else if prop_name == "linux,initrd-end" {
                            initrd_end = Some(read_be64(data) as usize);
                        }
                        if initrd_start.is_some() && initrd_end.is_some() {
                            let start = initrd_start.unwrap();
                            let end = initrd_end.unwrap();
                            let aligned_start = memory::align_down(start);
                            let aligned_end = memory::align_up(end);
                            let _ = reserved.push(memory::Region { start: aligned_start, end: aligned_end });
                            INITRD_START.store(start, Ordering::Relaxed);
                            INITRD_END.store(end, Ordering::Relaxed);
                            initrd_start = None;
                            initrd_end = None;
                        }
                    }
                    // check address and size cells
                    else if depth == 1 && prop_name == "#address-cells" {
                        let addr_cells = read_be32(data);
                        assert_eq!(addr_cells, 2, "DTB must have #address-cells = 2");
                    } else if depth == 1 && prop_name == "#size-cells" {
                        let size_cells = read_be32(data);
                        assert_eq!(size_cells, 2, "DTB must have #size-cells = 2");
                    }
                }
            }
            _ => {}
        }
    });

    memory
}
pub fn get_cpu_type() -> CpuType {
    match CPU_TYPE.load(Ordering::Relaxed) {
        1 => CpuType::Qemu,
        2 => CpuType::LicheeRVNano,
        _ => CpuType::Unknown,
    }
}

pub fn detect_cpu_type(dtb: *const u8) {
    if get_cpu_type() != CpuType::Unknown {
        return;
    }

    let ctx = unsafe { parse_dtb(dtb) };

    let mut found = false;

    unsafe {
        traverse_dtb(&ctx, |token, depth, _node_name, prop| {
            if found {
                return;
            }

            if let (DtbToken::Prop, 1, Some(("compatible", data, len))) = (token, depth, prop) {
                let slice = core::slice::from_raw_parts(data, len);
                if let Ok(compat_str) = core::str::from_utf8(slice) {
                    if compat_str.contains("cv181x") {
                        CPU_TYPE.store(CpuType::LicheeRVNano as u8, Ordering::Relaxed);
                        found = true;
                    }
                }
            }
        });
    }

    if !found {
        CPU_TYPE.store(CpuType::Qemu as u8, Ordering::Relaxed);
    }
}

#[allow(dead_code)]
pub unsafe fn print_dtb(dtb: *const u8) {
    let token_type = false;
    let ctx = parse_dtb(dtb);

    traverse_dtb(&ctx, |token, depth, name_opt, prop_opt| match token {
        DtbToken::BeginNode => {
            if let Some(name) = name_opt {
                for _ in 0..depth {
                    print!("  ");
                }
                if token_type {
                    print!("N{}: ", depth);
                }
                println!("{}", name);
            }
        }
        DtbToken::EndNode => {}
        DtbToken::Prop => {
            if let Some((name, data, len)) = prop_opt {
                for _ in 0..depth {
                    print!("  ");
                }
                if len == 4 {
                    let val = read_be32(data);
                    if token_type {
                        print!("P{}: ", depth);
                    }
                    println!("{} = 0x{:08x}", name, val);
                } else if len % 8 == 0 {
                    if token_type {
                        print!("P{}: ", depth);
                    }
                    print!("{} =", name);
                    for i in 0..(len / 8) {
                        let val = read_be64(data.add(i * 8));
                        print!(" 0x{:016x}", val);
                    }
                    println!();
                } else if len > 0 && *data.add(len - 1) == 0 {
                    let s =
                        core::str::from_utf8_unchecked(core::slice::from_raw_parts(data, len - 1));
                    if token_type {
                        print!("P{}: ", depth);
                    }
                    println!("{} = \"{}\"", name, s);
                } else {
                    if token_type {
                        print!("P{}: ", depth);
                    }
                    print!("{} =", name);
                    for i in 0..len {
                        print!(" {:02x}", *data.add(i));
                    }
                    println!();
                }
            }
        }
        _ => {}
    });
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
