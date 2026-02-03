use crate::initramfs;
use crate::process;
use crate::process_memory_map;
use crate::process_trampoline;

/// Load the init process ELF and enter user mode.
/// Assumes initramfs::init() has already been called.
/// Never returns.
pub fn run_init_process() -> ! {
    let mut handle = initramfs::ifs_open("/prog_example").expect("Failed to open /prog_example");

    process::init();

    let context = process::create();
    process_memory_map::init_from_elf(&mut handle, context);

    println!("Process context initialized, entering user mode");

    unsafe {
        process_trampoline::enter_process(context);
    }
}
