use memoffset::offset_of;
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use types::{TrampolineTrapFrame, ProcessTrapFrame, KernelTrapFrame, GpRegisters, ThreadContext};

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let mut asm = File::create(out_dir.join("offsets.S")).unwrap();

    writeln!(asm, "// Auto-generated file - DO NOT EDIT").unwrap();

    generate_trampoline_trap_frame_offsets(&mut asm);

    generate_process_trap_frame_offsets(&mut asm);

    generate_kernel_trap_frame_offsets(&mut asm);

    generate_thread_context_offsets(&mut asm);
}

fn generate_trampoline_trap_frame_offsets(asm: &mut File) {
    macro_rules! def_ttf {
        ($name:expr) => {
            writeln!(asm, ".equ TTF_{}, {}", stringify!($name).to_uppercase(), offset_of!(TrampolineTrapFrame, $name)).unwrap();
        };
    }

    def_ttf!(user_sp);
    def_ttf!(t0);
    def_ttf!(kernel_satp);
    def_ttf!(is_lichee_rvnano);
    def_ttf!(kernel_sp);
}

fn generate_process_trap_frame_offsets(asm: &mut File) {
    macro_rules! def_ptf {
        ($name:expr) => {
            writeln!(asm, ".equ PTF_{}, {}", stringify!($name).to_uppercase(), offset_of!(ProcessTrapFrame, $name)).unwrap();
        };
    }

    macro_rules! def_ptf_reg {
        ($name:expr) => {
            let reg_offset = offset_of!(ProcessTrapFrame, registers);
            writeln!(asm, ".equ PTF_{}, {}", stringify!($name).to_uppercase(), reg_offset + offset_of!(GpRegisters, $name)).unwrap();
        };
    }

    def_ptf!(pc);
    def_ptf!(sepc);
    def_ptf!(sstatus);
    def_ptf!(stval);
    def_ptf!(scause);
    def_ptf!(satp);

    def_ptf_reg!(ra);
    def_ptf_reg!(sp);
    def_ptf_reg!(gp);
    def_ptf_reg!(tp);
    def_ptf_reg!(t0);
    def_ptf_reg!(t1);
    def_ptf_reg!(t2);
    def_ptf_reg!(s0);
    def_ptf_reg!(s1);
    def_ptf_reg!(a0);
    def_ptf_reg!(a1);
    def_ptf_reg!(a2);
    def_ptf_reg!(a3);
    def_ptf_reg!(a4);
    def_ptf_reg!(a5);
    def_ptf_reg!(a6);
    def_ptf_reg!(a7);
    def_ptf_reg!(s2);
    def_ptf_reg!(s3);
    def_ptf_reg!(s4);
    def_ptf_reg!(s5);
    def_ptf_reg!(s6);
    def_ptf_reg!(s7);
    def_ptf_reg!(s8);
    def_ptf_reg!(s9);
    def_ptf_reg!(s10);
    def_ptf_reg!(s11);
    def_ptf_reg!(t3);
    def_ptf_reg!(t4);
    def_ptf_reg!(t5);
    def_ptf_reg!(t6);
}

fn generate_kernel_trap_frame_offsets(asm: &mut File) {
    macro_rules! def_ktf {
        ($name:expr) => {
            writeln!(asm, ".equ KTF_{}, {}", stringify!($name).to_uppercase(), offset_of!(KernelTrapFrame, $name)).unwrap();
        };
    }

    macro_rules! def_ktf_reg {
        ($name:expr) => {
            let reg_offset = offset_of!(KernelTrapFrame, registers);
            writeln!(asm, ".equ KTF_{}, {}", stringify!($name).to_uppercase(), reg_offset + offset_of!(GpRegisters, $name)).unwrap();
        };
    }

    def_ktf!(sepc);

    def_ktf_reg!(ra);
    def_ktf_reg!(sp);
    def_ktf_reg!(gp);
    def_ktf_reg!(tp);
    def_ktf_reg!(t0);
    def_ktf_reg!(t1);
    def_ktf_reg!(t2);
    def_ktf_reg!(s0);
    def_ktf_reg!(s1);
    def_ktf_reg!(a0);
    def_ktf_reg!(a1);
    def_ktf_reg!(a2);
    def_ktf_reg!(a3);
    def_ktf_reg!(a4);
    def_ktf_reg!(a5);
    def_ktf_reg!(a6);
    def_ktf_reg!(a7);
    def_ktf_reg!(s2);
    def_ktf_reg!(s3);
    def_ktf_reg!(s4);
    def_ktf_reg!(s5);
    def_ktf_reg!(s6);
    def_ktf_reg!(s7);
    def_ktf_reg!(s8);
    def_ktf_reg!(s9);
    def_ktf_reg!(s10);
    def_ktf_reg!(s11);
    def_ktf_reg!(t3);
    def_ktf_reg!(t4);
    def_ktf_reg!(t5);
    def_ktf_reg!(t6);

    // Size of the frame for stack allocation
    writeln!(asm, ".equ KTF_SIZE, {}", core::mem::size_of::<KernelTrapFrame>()).unwrap();
}

fn generate_thread_context_offsets(asm: &mut File) {
    macro_rules! def_tc {
        ($name:expr) => {
            writeln!(asm, ".equ TC_{}, {}", stringify!($name).to_uppercase(), offset_of!(ThreadContext, $name)).unwrap();
        };
    }

    def_tc!(sp);
    def_tc!(ra);
    def_tc!(s0);
    def_tc!(s1);
    def_tc!(s2);
    def_tc!(s3);
    def_tc!(s4);
    def_tc!(s5);
    def_tc!(s6);
    def_tc!(s7);
    def_tc!(s8);
    def_tc!(s9);
    def_tc!(s10);
    def_tc!(s11);
}
