use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use memoffset::offset_of;
use cpu_types::{Registers, TrapFrame};

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let dest_path = out_dir.join("trap_offsets.S");
    let mut f = File::create(&dest_path).unwrap();

    let mut rf = File::create(out_dir.join("trap_offsets.rs")).unwrap();

    writeln!(f, "// Auto-generated file - DO NOT EDIT").unwrap();
    writeln!(rf, "// Auto-generated file - DO NOT EDIT").unwrap();

    macro_rules! def {
        ($name:expr, $val:expr) => {
            writeln!(f, ".equ {}, {}", $name, $val).unwrap();
            writeln!(rf, "pub const {}: usize = {};", $name, $val).unwrap();
        };
    }

    def("TF_RA", offset_of!(TrapFrame, registers.ra));
    def("TF_SP", offset_of!(TrapFrame, registers.sp));
    def("TF_GP", offset_of!(TrapFrame, registers.gp));
    def("TF_TP", offset_of!(TrapFrame, registers.tp));
    def("TF_T0", offset_of!(TrapFrame, registers.t0));
    def("TF_T1", offset_of!(TrapFrame, registers.t1));
    def("TF_T2", offset_of!(TrapFrame, registers.t2));
    def("TF_S0", offset_of!(TrapFrame, registers.s0));
    def("TF_S1", offset_of!(TrapFrame, registers.s1));
    def("TF_A0", offset_of!(TrapFrame, registers.a0));
    def("TF_A1", offset_of!(TrapFrame, registers.a1));
    def("TF_A2", offset_of!(TrapFrame, registers.a2));
    def("TF_A3", offset_of!(TrapFrame, registers.a3));
    def("TF_A4", offset_of!(TrapFrame, registers.a4));
    def("TF_A5", offset_of!(TrapFrame, registers.a5));
    def("TF_A6", offset_of!(TrapFrame, registers.a6));
    def("TF_A7", offset_of!(TrapFrame, registers.a7));
    def("TF_S2", offset_of!(TrapFrame, registers.s2));
    def("TF_S3", offset_of!(TrapFrame, registers.s3));
    def("TF_S4", offset_of!(TrapFrame, registers.s4));
    def("TF_S5", offset_of!(TrapFrame, registers.s5));
    def("TF_S6", offset_of!(TrapFrame, registers.s6));
    def("TF_S7", offset_of!(TrapFrame, registers.s7));
    def("TF_S8", offset_of!(TrapFrame, registers.s8));
    def("TF_S9", offset_of!(TrapFrame, registers.s9));
    def("TF_S10", offset_of!(TrapFrame, registers.s10));
    def("TF_S11", offset_of!(TrapFrame, registers.s11));
    def("TF_T3", offset_of!(TrapFrame, registers.t3));
    def("TF_T4", offset_of!(TrapFrame, registers.t4));
    def("TF_T5", offset_of!(TrapFrame, registers.t5));
    def("TF_T6", offset_of!(TrapFrame, registers.t6));
    def("TF_PC", offset_of!(TrapFrame, registers.pc));

    def("TF_SEPC", offset_of!(TrapFrame, sepc));
    def("TF_SSTATUS", offset_of!(TrapFrame, sstatus));
    def("TF_STVAL", offset_of!(TrapFrame, stval));
    def("TF_SCAUSE", offset_of!(TrapFrame, scause));
    def("TF_KERNEL_SATP", offset_of!(TrapFrame, kernel_satp));
    def("TF_PROCESS_SATP", offset_of!(TrapFrame, process_satp));
    def("TF_IS_LICHEE_RVNANO", offset_of!(TrapFrame, is_lichee_rvnano));
}
