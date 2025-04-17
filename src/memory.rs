use heapless;

#[derive(Debug, Copy, Clone)]
pub struct Region {
    pub start: usize,
    pub end: usize,
}

pub const PAGE_SIZE: usize = 4096;

#[inline]
pub fn align_down(addr: usize) -> usize {
    addr & !(PAGE_SIZE - 1)
}

#[inline]
pub fn align_up(addr: usize) -> usize {
    (addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}

/// Compute the list of usable (non‑reserved) subregions within `ram`.
///
/// - `ram` is the full, addressable RAM: `[ram.start, ram.end)`.
/// - `reserved` is an unordered Vec of reserved ranges (some may overlap, or lie
///   outside `ram`).
/// - `usable` will be filled (in order) with the disjoint segments of RAM not
///   covered by any reserved region.  Its capacity (`U`) must be large enough
///   to hold the worst‑case number of fragments (e.g. reserved.len()+1).
pub fn compute_usable_regions<const R: usize, const U: usize>(
    ram: Region,
    reserved: &heapless::Vec<Region, R>,
    usable: &mut heapless::Vec<Region, U>,
) {
    // 1) Clip each reserved region to [ram.start, ram.end), drop empties.
    let mut clipped: heapless::Vec<Region, R> = heapless::Vec::new();
    for &r in reserved.iter() {
        let s = r.start.max(ram.start);
        let e = r.end.min(ram.end);
        if s < e {
            let _ = clipped.push(Region { start: s, end: e });
        }
    }

    // 2) Sort clipped by start (simple O(n²) for small R).
    for i in 0..clipped.len() {
        for j in (i + 1)..clipped.len() {
            if clipped[j].start < clipped[i].start {
                clipped.swap(i, j);
            }
        }
    }

    // 3) Merge overlapping/adjacent into `merged`.
    let mut merged: heapless::Vec<Region, R> = heapless::Vec::new();
    for &r in clipped.iter() {
        if let Some(last) = merged.last_mut() {
            if r.start <= last.end {
                // overlap or touch ⇒ extend end if needed
                if r.end > last.end {
                    last.end = r.end;
                }
                continue;
            }
        }
        let _ = merged.push(r);
    }

    // 4) Carve out the gaps between `ram.start` and `ram.end`
    let mut cursor = ram.start;
    for &r in merged.iter() {
        if r.start > cursor {
            let _ = usable.push(Region { start: cursor, end: r.start });
        }
        cursor = cursor.max(r.end);
    }
    if cursor < ram.end {
        let _ = usable.push(Region { start: cursor, end: ram.end });
    }
}

/*
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_align_zero() {
        assert_eq!(align_down(0), 0);
        assert_eq!(align_up  (0), 0);
    }

    #[test]
    fn test_page_examples() {
        assert_eq!(align_down(0x1003), 0x1000);
        assert_eq!(align_up(0x1003), 0x2000);

        // already on page boundary
        assert_eq!(align_down(0x3000), 0x3000);
        assert_eq!(align_up(0x3000), 0x3000);
    }
}
*/
