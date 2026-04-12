use core::mem::transmute;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use crate::dtb;

const QEMU_PLIC_BASE: usize = 0x0c00_0000;
const NANO_PLIC_BASE: usize = 0x7000_0000;

// PLIC register offsets from base address
const PRIORITY_OFFSET: usize = 0x0000;      // Priority registers base
const ENABLE_OFFSET: usize = 0x2000;        // Enable registers base
const PLIC_CONTEXT_BASE: usize = 0x200000;       // Context registers base
const PLIC_CONTEXT_STRIDE: usize = 0x1000;       // Bytes between contexts
const CLAIM_COMPLETE_OFFSET: usize = 0x4;   // Claim/complete register offset from context

// S-mode context for hart 0
// Context ID: 0 = M-mode hart 0, 1 = S-mode hart 0, 2 = M-mode hart 1, etc.
const PLIC_S_MODE_CONTEXT_ID: usize = 1;
const PLIC_S_CONTEXT: usize = PLIC_CONTEXT_BASE + PLIC_CONTEXT_STRIDE * PLIC_S_MODE_CONTEXT_ID;

// Context enable register stride (per hart/mode combination)
const PLIC_ENABLE_CONTEXT_STRIDE: usize = 0x80;

// Maximum IRQ number we support (covers UART at 44, SD at 36, virtio at 1-8)
const MAX_IRQ: usize = 64;

static PLIC_BASE: AtomicUsize = AtomicUsize::new(0);

// IRQ handler lookup table, indexed by IRQ number.
// All handlers have signature fn(u32) where the parameter is the IRQ number.
// Stored as *mut () because Rust has no atomic function pointer type. On RISC-V,
// fn(u32) and *mut () are both 64-bit, so the transmute in dispatch_irq is valid.
static IRQ_HANDLERS: [AtomicPtr<()>; MAX_IRQ] = [const { AtomicPtr::new(core::ptr::null_mut()) }; MAX_IRQ];

pub unsafe fn init() {
    // Dynamically select PLIC base based on CPU type
    let plic_base = match dtb::get_cpu_type() {
        dtb::CpuType::Qemu => QEMU_PLIC_BASE,
        dtb::CpuType::LicheeRVNano => NANO_PLIC_BASE,
        _ => {
            println!("WARNING: Unknown CPU type, defaulting to QEMU");
            QEMU_PLIC_BASE
        }
    };

    PLIC_BASE.store(plic_base, Ordering::Relaxed);

    println!("PLIC: initialized at base {:#x}", plic_base);

    // Set priority threshold to 0 to allow all interrupt priorities
    let threshold_reg = (plic_base + PLIC_S_CONTEXT) as *mut u32;
    threshold_reg.write_volatile(0);

    // Note: Individual IRQs are now enabled via register_irq() by their respective drivers
}

/// Claim the highest-priority pending interrupt for S-mode hart 0.
/// Returns the IRQ number, or 0 if no interrupt is pending.
fn claim() -> u32 {
    let base = PLIC_BASE.load(Ordering::Relaxed);
    if base == 0 {
        return 0;
    }
    let claim_reg = (base + PLIC_S_CONTEXT + CLAIM_COMPLETE_OFFSET) as *const u32;
    unsafe { claim_reg.read_volatile() }
}

/// Signal completion of interrupt handling for the given IRQ.
fn complete(irq: u32) {
    let base = PLIC_BASE.load(Ordering::Relaxed);
    if base == 0 {
        return;
    }
    let complete_reg = (base + PLIC_S_CONTEXT + CLAIM_COMPLETE_OFFSET) as *mut u32;
    unsafe { complete_reg.write_volatile(irq) }
}

/// Dispatch an external interrupt. Claims the IRQ, calls the appropriate
/// driver handler, then completes the IRQ.
pub fn dispatch_irq() {
    let irq = claim();
    if irq == 0 {
        return;
    }

    if (irq as usize) >= MAX_IRQ {
        println!("PLIC: IRQ {} exceeds MAX_IRQ {}", irq, MAX_IRQ);
        complete(irq);
        return;
    }

    let handler_ptr = IRQ_HANDLERS[irq as usize].load(Ordering::Relaxed);
    if !handler_ptr.is_null() {
        // SAFETY: handler_ptr was written by register_irq via `handler as usize as *mut ()`.
        // The value is a valid fn(u32) pointer; transmute recovers it from *mut ().
        let handler: fn(u32) = unsafe { transmute(handler_ptr) };
        handler(irq);
    } else {
        println!("PLIC: IRQ {} but no handler registered", irq);
    }

    complete(irq);
}


/// Register an IRQ handler and enable the IRQ in one operation.
/// This combines handler registration with IRQ enabling for cleaner driver code.
/// Handler signature is fn(u32) where the parameter is the IRQ number.
pub fn register_irq(irq_id: u32, handler: fn(u32)) {
    let irq = irq_id as usize;
    if irq >= MAX_IRQ {
        println!("PLIC: IRQ {} exceeds MAX_IRQ {}", irq_id, MAX_IRQ);
        return;
    }

    // Store handler in lookup table
    IRQ_HANDLERS[irq].store(handler as usize as *mut (), Ordering::Relaxed);

    // Enable the IRQ in PLIC hardware
    unsafe {
        let base = PLIC_BASE.load(Ordering::Relaxed);
        if base == 0 {
            println!("PLIC: Cannot enable IRQ {} - PLIC not initialized", irq_id);
            return;
        }

        // Set interrupt priority to 1 (priority 0 means disabled)
        let priority_reg = (base + PRIORITY_OFFSET + (irq_id as usize) * 4) as *mut u32;
        priority_reg.write_volatile(1);

        // Enable interrupt for S-mode hart 0
        let enable_base = base + ENABLE_OFFSET + PLIC_ENABLE_CONTEXT_STRIDE * PLIC_S_MODE_CONTEXT_ID;
        let word_index = (irq_id / 32) as usize;
        let irq_bit = 1 << (irq_id % 32);
        let enable_word = (enable_base + word_index * 4) as *mut u32;

        let current = enable_word.read_volatile();
        enable_word.write_volatile(current | irq_bit);
    }
}
