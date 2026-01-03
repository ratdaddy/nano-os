use crate::process;

pub fn handle_amo_fault(trap_frame: &mut types::ProcessTrapFrame) {
    println!("Handling AMO fault");
    let process_context = process::Context::current();
    let phys_sepc = process_context.page_map.virt_to_phys(trap_frame.sepc).unwrap();
    let inst = unsafe { core::ptr::read_unaligned(phys_sepc as *const u32) };

    println!("Handling AMO fault, phys sepc: {:#x}, inst: {:#x}", phys_sepc, inst);
    println!("instruction decode: opcode: {:#x}, funct3: {:#x}, funct5: {:#x}", inst & 0x7f, (inst >> 12) & 0x7, (inst >> 27) & 0x1f);

    // Check if this is an amoswap.w instruction
    if (inst & 0x7f) == 0x2f && ((inst >> 12) & 0x3) == 0x2 && ((inst >> 27) & 0x1f) == 0x1 {
        let rd = (inst >> 7) & 0x1f;
        let rs1 = (inst >> 15) & 0x1f;
        let rs2 = (inst >> 20) & 0x1f;
        println!("AMO instruction detected: amoswap.w, rd: {}, rs1: {}, rs2: {}", rd, rs1, rs2);

        let virt_addr = trap_frame.registers.get(rs1 as usize);
        let phys_addr = process_context.page_map.virt_to_phys(virt_addr).unwrap();
        let val = trap_frame.registers.get(rs2 as usize) as u32;
        let prev = unsafe { core::ptr::read_volatile(phys_addr as *const u32) };
        unsafe { core::ptr::write_volatile(phys_addr as *mut u32, val) };
        println!("AMO operation: wrote {:#x} to address {:#x}, previous value was {:#x}", val, phys_addr, prev);
        *trap_frame.registers.get_mut(rd as usize) = prev as usize;

        trap_frame.pc = trap_frame.sepc + 4; // advance past instruction
    } else {
        panic!("Unhandled AMO or illegal instruction");
    }
}
