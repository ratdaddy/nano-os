use crate::process;

pub fn handle_amo_fault(trap_frame: &mut types::ProcessTrapFrame) {
    #[cfg(feature = "trace_amo")]
    println!("Handling AMO fault");
    let process_context = process::Context::current();
    let phys_sepc = process_context.page_map.virt_to_phys(trap_frame.sepc).unwrap();
    let inst = unsafe { core::ptr::read_unaligned(phys_sepc as *const u32) };

    #[cfg(feature = "trace_amo")]
    {
        println!("Handling AMO fault, phys sepc: {:#x}, inst: {:#x}", phys_sepc, inst);
        println!("instruction decode: opcode: {:#x}, funct3: {:#x}, funct5: {:#x}", inst & 0x7f, (inst >> 12) & 0x7, (inst >> 27) & 0x1f);
    }

    let opcode = inst & 0x7f;
    let funct3 = (inst >> 12) & 0x7;
    let funct5 = (inst >> 27) & 0x1f;
    let rd = (inst >> 7) & 0x1f;
    let rs1 = (inst >> 15) & 0x1f;
    let rs2 = (inst >> 20) & 0x1f;

    if opcode != 0x2f {
        panic!("Not an AMO instruction: opcode {:#x}", opcode);
    }

    let virt_addr = trap_frame.registers.get(rs1 as usize);
    let phys_addr = process_context.page_map.virt_to_phys(virt_addr).unwrap();

    match (funct5, funct3) {
        // amoswap.w
        (0x1, 0x2) => {
            #[cfg(feature = "trace_amo")]
            println!("AMO instruction: amoswap.w, rd: {}, rs1: {}, rs2: {}", rd, rs1, rs2);
            let val = trap_frame.registers.get(rs2 as usize) as u32;
            let prev = unsafe { core::ptr::read_volatile(phys_addr as *const u32) };
            unsafe { core::ptr::write_volatile(phys_addr as *mut u32, val) };
            #[cfg(feature = "trace_amo")]
            println!("AMO operation: wrote {:#x} to address {:#x}, previous value was {:#x}", val, phys_addr, prev);
            *trap_frame.registers.get_mut(rd as usize) = prev as usize;
        }
        // amoadd.w
        (0x0, 0x2) => {
            #[cfg(feature = "trace_amo")]
            println!("AMO instruction: amoadd.w, rd: {}, rs1: {}, rs2: {}", rd, rs1, rs2);
            let addend = trap_frame.registers.get(rs2 as usize) as u32;
            let prev = unsafe { core::ptr::read_volatile(phys_addr as *const u32) };
            let result = prev.wrapping_add(addend);
            unsafe { core::ptr::write_volatile(phys_addr as *mut u32, result) };
            #[cfg(feature = "trace_amo")]
            println!("AMO operation: added {:#x} to {:#x} -> {:#x} at address {:#x}", addend, prev, result, phys_addr);
            *trap_frame.registers.get_mut(rd as usize) = prev as usize;
        }
        // amoadd.d
        (0x0, 0x3) => {
            #[cfg(feature = "trace_amo")]
            println!("AMO instruction: amoadd.d, rd: {}, rs1: {}, rs2: {}", rd, rs1, rs2);
            let addend = trap_frame.registers.get(rs2 as usize) as u64;
            let prev = unsafe { core::ptr::read_volatile(phys_addr as *const u64) };
            let result = prev.wrapping_add(addend);
            unsafe { core::ptr::write_volatile(phys_addr as *mut u64, result) };
            #[cfg(feature = "trace_amo")]
            println!("AMO operation: added {:#x} to {:#x} -> {:#x} at address {:#x}", addend, prev, result, phys_addr);
            *trap_frame.registers.get_mut(rd as usize) = prev as usize;
        }
        _ => {
            panic!("Unhandled AMO instruction: funct5={:#x}, funct3={:#x}", funct5, funct3);
        }
    }

    trap_frame.pc = trap_frame.sepc + 4;
}
