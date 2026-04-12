# Hardware-Shared Buffer Alignment

## The Problem

When a physical address is handed to a hardware device — for DMA, for a
shared ring buffer, or for any other memory-mapped control structure — the
device sees physical memory directly. The kernel's virtual address space is
invisible to it.

The page allocator guarantees physical contiguity *within* a page but not
*across* page boundaries. Two adjacent virtual addresses can map to
physically discontiguous pages. A buffer that straddles a page boundary
therefore has a seam where the physical address jumps discontinuously. A
device accessing across that seam will read or write the wrong memory.

## The Rule

A hardware-shared buffer of size S must be aligned to at least S, rounded up
to the next power of two. A power-of-two-aligned allocation of size S starts
at an address that is a multiple of some N ≥ S, where N is itself a power of
two. The next multiple of N is exactly N bytes away — farther than S — so the
buffer is guaranteed to end before the next N-aligned boundary. It is
contained within one aligned block and cannot cross a page boundary.

Without this property you cannot reason about containment from size alone:
a non-power-of-two-aligned buffer could start at any offset within a page, and
you would need to know the allocator's exact placement to determine whether it
stays within the page.

The alignment must also divide PAGE_SIZE. If it did not, the guarantee would
not hold — an allocation larger than a page can span physically discontiguous
pages regardless of alignment.

Concretely: `align = min(next_power_of_two(size), PAGE_SIZE)`.

This rule holds for the current allocator. An allocator that tracked physical
contiguity across pages and exposed that guarantee could allow larger buffers
without requiring power-of-two alignment.

## Allocating Hardware-Shared Buffers

Use `alloc_within_page<T>()` for all hardware-shared buffers. This function
derives the required alignment automatically from `size_of::<T>()`,
eliminating manually chosen alignment constants and wrapper types.

Prefer concrete array or struct types as the type parameter — the size, and
therefore the alignment, is then determined by the type itself rather than a
separate constant that can fall out of sync.

Do not use `Box::new` for hardware-shared buffers. The standard allocator
provides only natural type alignment, which is typically less than the size
of the buffer.

**Prefer heap allocation over static.** Static hardware-shared buffers require
a manually computed alignment constant in a `repr(C, align(N))` wrapper, which
can fall out of sync with the buffer size. `alloc_within_page` derives
alignment from the type automatically and eliminates the wrapper. Use static
allocation only when the buffer must be ready before the heap is initialized.

## What This Does Not Cover

- **Protocol-required alignment**: some types carry alignment for hardware
  protocol reasons independent of page-boundary safety. For example, VirtqDesc
  requires 16-byte alignment per the VirtIO spec; PageTable requires 4096-byte
  alignment for the RISC-V MMU. These alignments describe the structure of a
  hardware interface, not an allocation strategy.

- **Non-contiguous disk sectors**: alignment prevents buffer fragmentation; it
  does not make non-contiguous disk sectors addressable as one transfer. A
  scatter-gather descriptor list (e.g., ADMA2) handles that separately.

- **Cache coherency**: a separate concern on non-coherent architectures.
  Flush before write DMA; flush-and-invalidate after read DMA.
