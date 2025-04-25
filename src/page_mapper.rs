#![allow(dead_code)]

use core::ops::BitOr;

use crate::memory;
use crate::page_allocator;

const PAGE_ENTRIES: usize = 512;

unsafe fn zero_page_table(ptr: *mut PageTable) {
    core::ptr::write_bytes(ptr, 0, 1);
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PageSize {
    Size4K,
    Size2M,
    Size1G,
}

impl PageSize {
    pub const fn size(&self) -> usize {
        match self {
            PageSize::Size4K => 4 * 1024,
            PageSize::Size2M => 2 * 1024 * 1024,
            PageSize::Size1G => 1024 * 1024 * 1024,
        }
    }

    /// Returns the Sv39 page table level this size maps at.
    ///
    /// - L0 (0): 4 KiB
    /// - L1 (1): 2 MiB
    /// - L2 (2): 1 GiB
    pub const fn level(&self) -> usize {
        match self {
            PageSize::Size4K => 0,
            PageSize::Size2M => 1,
            PageSize::Size1G => 2,
        }
    }
}

#[derive(Copy, Clone)]
pub struct PageFlags {
    bits: usize,
}

impl PageFlags {
    pub const VALID: Self = Self { bits: 1 << 0 };
    pub const READ: Self = Self { bits: 1 << 1 };
    pub const WRITE: Self = Self { bits: 1 << 2 };
    pub const EXECUTE: Self = Self { bits: 1 << 3 };
    pub const USER: Self = Self { bits: 1 << 4 };
    pub const GLOBAL: Self = Self { bits: 1 << 5 };
    pub const ACCESSED: Self = Self { bits: 1 << 6 };
    pub const DIRTY: Self = Self { bits: 1 << 7 };

    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    pub fn bits(self) -> usize {
        self.bits
    }

    pub fn intersects(&self, other: PageFlags) -> bool {
        self.bits() & other.bits() != 0
    }
}

impl BitOr for PageFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self { bits: self.bits | rhs.bits }
    }
}

#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct PageTableEntry(usize);

impl PageTableEntry {
    pub fn new() -> Self {
        Self(0)
    }

    pub fn set(&mut self, phys_addr: usize, flags: PageFlags) {
        let ppn = phys_to_ppn(phys_addr);
        let page_flags = flags | PageFlags::VALID;
        self.0 = (ppn << 10) | page_flags.bits();
    }

    pub fn is_valid(&self) -> bool {
        self.0 & PageFlags::VALID.bits() != 0
    }

    pub fn addr(&self) -> usize {
        (self.0 >> 10) << 12
    }

    pub fn flags(&self) -> PageFlags {
        PageFlags { bits: self.0 & 0x3ff }
    }

    pub fn raw(&self) -> usize {
        self.0
    }
}

fn phys_to_ppn(addr: usize) -> usize {
    (addr >> 12) & 0x000f_ffff_ffff // Sv39: 44 bits
}

pub trait VirtualAddressExt {
    fn vpn2(self) -> usize;
    fn vpn1(self) -> usize;
    fn vpn0(self) -> usize;
}

impl VirtualAddressExt for usize {
    fn vpn2(self) -> usize {
        (self >> 30) & 0x1ff
    }

    fn vpn1(self) -> usize {
        (self >> 21) & 0x1ff
    }

    fn vpn0(self) -> usize {
        (self >> 12) & 0x1ff
    }
}

#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [PageTableEntry; PAGE_ENTRIES],
}

#[derive(Clone, Copy, Debug)]
pub struct PageMapper {
    pub root_table: *mut PageTable,
}

impl PageMapper {
    pub fn new() -> Self {
        let root_frame = page_allocator::alloc().expect("Failed to allocate root page table");
        let root_ptr = root_frame as *mut PageTable;

        unsafe {
            zero_page_table(root_ptr);
        }

        Self { root_table: root_ptr }
    }

    pub fn map_range(
        &self,
        virt_start: usize,
        phys_start: usize,
        end: usize,
        flags: PageFlags,
        page_size: PageSize,
    ) {
        let step = page_size.size();
        let leaf_level = page_size.level();

        assert!(virt_start % step == 0, "virtual address not aligned");
        assert!(phys_start % step == 0, "physical address not aligned");
        assert!(end % step == 0, "end address not aligned");

        let mut offset = 0;
        let size = end - virt_start;

        fn alloc_next_level(parent_entry: &mut PageTableEntry) -> *mut PageTable {
            let new_frame = page_allocator::alloc().expect("Out of memory for page table");
            let new_table = new_frame as *mut PageTable;

            unsafe {
                zero_page_table(new_table);
            }

            parent_entry.set(new_frame, PageFlags::empty());
            new_table
        }

        while offset < size {
            let virt = virt_start + offset;
            let phys = phys_start + offset;

            let mut table = self.root_table;

            // L2
            let l2_idx = virt.vpn2();
            let l2_entry = unsafe { &mut (*table).entries[l2_idx] };

            if leaf_level == 2 {
                l2_entry.set(phys, flags);
                offset += step;
                continue;
            }

            if !l2_entry.is_valid() {
                table = alloc_next_level(l2_entry);
            } else {
                table = l2_entry.addr() as *mut PageTable;
            }

            // L1
            let l1_idx = virt.vpn1();
            let l1_entry = unsafe { &mut (*table).entries[l1_idx] };

            if leaf_level == 1 {
                l1_entry.set(phys, flags);
                offset += step;
                continue;
            }

            if !l1_entry.is_valid() {
                table = alloc_next_level(l1_entry);
            } else {
                table = l1_entry.addr() as *mut PageTable;
            }

            // L0
            let l0_idx = virt.vpn0();
            let l0_entry = unsafe { &mut (*table).entries[l0_idx] };
            l0_entry.set(phys, flags);

            offset += step;
        }
    }

    pub fn allocate_and_map_pages(&self, virt: usize, size: usize, flags: PageFlags) {
        let page_count = size / memory::PAGE_SIZE;

        for i in 0..page_count {
            let phys = page_allocator::alloc().expect("Out of memory for page");
            let virt_addr = virt + i * memory::PAGE_SIZE;
            self.map_range(virt_addr, phys, virt_addr + memory::PAGE_SIZE, flags, PageSize::Size4K);
        }
    }

    pub fn dump_pte(&self, virt_addr: usize) {
        unsafe {
            let vpn2 = virt_addr.vpn2();
            let vpn1 = virt_addr.vpn1();

            let l2_entry = (*self.root_table).entries[vpn2];
            let l1_table = l2_entry.addr() as *const PageTable;

            let l1_entry = (*l1_table).entries[vpn1];
            let l0_table = l1_entry.addr() as *const PageTable;

            println!("Level 0 PTEs at {:p} for VA {:#x}", l0_table, virt_addr);

            for i in 0..PAGE_ENTRIES {
                let pte = (*l0_table).entries[i];
                if pte.addr() != 0 {
                    let ppn = pte.addr();
                    let flags = pte.flags().bits();
                    let va = (vpn2 << 30) | (vpn1 << 21) | (i << 12);
                    println!(
                        "VA {:#013x} => PPN {:#x}, flags {:#x}",
                        va | 0xffff_ff10_0000_0000,
                        ppn,
                        flags
                    );
                }
            }
        }
    }

    pub fn dump_vmmap(&self) {
        unsafe {
            println!("Dumping virtual memory map from root at {:p}", self.root_table);
            Self::walk_table(self.root_table as *const PageTable, 2, 0);
        }
    }

    unsafe fn walk_table(table: *const PageTable, level: usize, virt_base: usize) {
        fn sign_extend_sv39(va: usize) -> usize {
            const BITS: usize = 39;
            let shift = 64 - BITS;
            (va << shift) >> shift
        }

        for i in 0..PAGE_ENTRIES {
            let pte = (*table).entries[i];
            if pte.addr() == 0 || !pte.is_valid() {
                continue;
            }

            let va = virt_base | (i << (12 + 9 * level));
            let canonical_va = sign_extend_sv39(va);

            if pte.flags().intersects(PageFlags::READ | PageFlags::WRITE | PageFlags::EXECUTE) {
                let size = 1 << (12 + 9 * level);
                println!(
                    "VA {:#013x}..{:#013x} => PPN {:#x}, flags {:#x} ({} KiB)",
                    canonical_va,
                    canonical_va + size - 1,
                    pte.addr(),
                    pte.flags().bits(),
                    size / 1024,
                );
            } else {
                let next_table = pte.addr() as *const PageTable;
                Self::walk_table(next_table, level - 1, va);
            }
        }
    }

    pub fn cloning_allocate_and_map_pages(&mut self, virt: usize, size: usize, flags: PageFlags) {
        let vpn2 = virt.vpn2();
        let vpn1 = virt.vpn1();

        assert!(size % memory::PAGE_SIZE == 0, "Size must be page aligned");
        assert!(virt % memory::PAGE_SIZE == 0, "Virtual address must be page aligned");
        assert!(vpn1 == (virt + size - 1).vpn1(), "Must not cross L1 (2 MiB) boundary");

        let page_count = size / memory::PAGE_SIZE;

        fn clone_or_zero_table(src: *mut PageTable) -> *mut PageTable {
            let dst =
                page_allocator::alloc().expect("Out of memory for page table") as *mut PageTable;
            unsafe {
                if src.is_null() {
                    core::ptr::write_bytes(dst, 0, 1);
                } else {
                    println!("Cloning page table at {src:?} to {dst:?}");
                    core::ptr::copy_nonoverlapping(src, dst, 1);
                    //page_allocator::dealloc(src as usize, 1);
                }
            }
            dst
        }

        unsafe {
            let old_l2 = self.root_table;
            println!("Old L2 page table at {:#x}", old_l2 as usize);
            let new_l2 = clone_or_zero_table(old_l2);

            let old_l1 = if (*old_l2).entries[vpn2].is_valid() {
                (*old_l2).entries[vpn2].addr() as *mut PageTable
            } else {
                core::ptr::null_mut()
            };

            println!("Old L1  page table at {:#x}", old_l1 as usize);
            let new_l1 = clone_or_zero_table(old_l1);
            (*new_l2).entries[vpn2].set(new_l1 as usize, PageFlags::empty());

            let old_l0 = if (*new_l1).entries[vpn1].is_valid() {
                (*new_l1).entries[vpn1].addr() as *mut PageTable
            } else {
                core::ptr::null_mut()
            };

            println!("Old L0 page table at {:#x}", old_l0 as usize);
            let new_l0 = clone_or_zero_table(old_l0);
            (*new_l1).entries[vpn1].set(new_l0 as usize, PageFlags::empty());

            for i in 0..page_count {
                let virt_addr = virt + i * memory::PAGE_SIZE;
                let phys = page_allocator::alloc().expect("Out of memory for page");
                let l0_idx = virt_addr.vpn0();
                (*new_l0).entries[l0_idx].set(phys, flags);
            }

            self.root_table = new_l2;
        }
    }
}
