//! Block device registry.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use core::fmt;
use spin::Mutex;

use crate::block::volume::BlockVolume;
use crate::drivers::BlockError;

struct BlkdevEntry {
    name: String,
    volume: Arc<dyn BlockVolume>,
}

impl fmt::Debug for BlkdevEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlkdevEntry").field("name", &self.name).finish_non_exhaustive()
    }
}

static BLOCKDEVS: Mutex<BTreeMap<(u32, u32), BlkdevEntry>> = Mutex::new(BTreeMap::new());

/// Register a block device for the given major/minor.
pub fn blkdev_register(major: u32, minor: u32, name: &str, volume: Arc<dyn BlockVolume>) {
    BLOCKDEVS.lock().insert((major, minor), BlkdevEntry {
        name: String::from(name),
        volume,
    });
}

/// Call `f` for each registered block device: (major, minor, name).
pub fn blkdev_for_each(mut f: impl FnMut(u32, u32, &str)) {
    for (&(major, minor), entry) in BLOCKDEVS.lock().iter() {
        f(major, minor, &entry.name);
    }
}

/// Look up a registered block device by major/minor and return a clone of its volume.
pub fn blkdev_get(major: u32, minor: u32) -> Result<Arc<dyn BlockVolume>, BlockError> {
    BLOCKDEVS.lock()
        .get(&(major, minor))
        .map(|e| Arc::clone(&e.volume))
        .ok_or(BlockError::InvalidInput)
}

#[cfg(test)]
pub fn blkdev_clear() {
    BLOCKDEVS.lock().clear();
}

#[cfg(test)]
mod tests {
    use alloc::format;
    use alloc::sync::Arc;

    use crate::block::volume::BlockVolume;
    use crate::drivers::BlockError;
    use super::*;

    struct MockVolume { num_sectors: u64 }

    impl BlockVolume for MockVolume {
        fn read_blocks(&self, _lba: u64, buf: &mut [u8]) -> Result<(), BlockError> {
            for b in buf.iter_mut() { *b = 0xab; }
            Ok(())
        }
        fn size_blocks(&self) -> u64 { self.num_sectors }
    }

    #[test_case]
    fn test_blkdev_register_and_get() {
        println!("Testing blkdev_register and blkdev_get...");

        let volume = Arc::new(MockVolume { num_sectors: 2048 });
        blkdev_register(8, 0, "sda", volume);

        let got = blkdev_get(8, 0).unwrap();
        assert_eq!(got.size_blocks(), 2048);
    }

    #[test_case]
    fn test_blkdev_get_not_registered() {
        println!("Testing blkdev_get on unregistered device returns InvalidInput...");

        let result = blkdev_get(99, 99);
        assert!(matches!(result, Err(BlockError::InvalidInput)));
    }

    #[test_case]
    fn test_blkdev_for_each() {
        println!("Testing blkdev_for_each visits registered devices...");

        let name = format!("sda{}", 4u32);
        blkdev_register(8, 4, &name, Arc::new(MockVolume { num_sectors: 512 }));

        let mut found = false;
        blkdev_for_each(|major, minor, name| {
            if major == 8 && minor == 4 && name == "sda4" {
                found = true;
            }
        });
        assert!(found);
    }
}
