#![no_std]

#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct Registers {
    pub ra: usize,
    pub sp: usize,
    pub gp: usize,
    pub tp: usize,
    pub t0: usize,
    pub t1: usize,
    pub t2: usize,
    pub s0: usize,
    pub s1: usize,
    pub a0: usize,
    pub a1: usize,
    pub a2: usize,
    pub a3: usize,
    pub a4: usize,
    pub a5: usize,
    pub a6: usize,
    pub a7: usize,
    pub s2: usize,
    pub s3: usize,
    pub s4: usize,
    pub s5: usize,
    pub s6: usize,
    pub s7: usize,
    pub s8: usize,
    pub s9: usize,
    pub s10: usize,
    pub s11: usize,
    pub t3: usize,
    pub t4: usize,
    pub t5: usize,
    pub t6: usize,
    pub pc: usize,
}

impl Registers {
    pub fn new() -> Self {
        Self::default()
    }
}

#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct TrapFrame {
    pub registers: Registers,
    pub sepc: usize,
    pub sstatus: usize,
    pub stval: usize,
    pub scause: usize,
    pub kernel_satp: usize,
    pub process_satp: usize,
    pub is_lichee_rvnano: usize,
}
