//! ext2 directory entry iteration

use alloc::string::String;
use alloc::sync::Arc;
use core::str::from_utf8;

use crate::block::volume::BlockBuf;
use crate::bytes::ReadIntLe;
use crate::drivers::BlockError;
use crate::file::Inode;

use super::inode::Ext2InodeData;
use super::superblock::Ext2SuperBlock;

// Directory entry structure offsets
const DIR_ENTRY_INODE_OFFSET: usize = 0;
const DIR_ENTRY_REC_LEN_OFFSET: usize = 4;
const DIR_ENTRY_NAME_LEN_OFFSET: usize = 6;
const DIR_ENTRY_FILE_TYPE_OFFSET: usize = 7;
const DIR_ENTRY_NAME_OFFSET: usize = 8;

// Directory entry file type values
pub(super) const EXT2_FT_REG_FILE: u8 = 1;
pub(super) const EXT2_FT_DIR: u8 = 2;
pub(super) const EXT2_FT_CHRDEV: u8 = 3;
pub(super) const EXT2_FT_BLKDEV: u8 = 4;

/// An iterator over raw directory entries in an ext2 directory inode.
///
/// Reads data blocks one at a time (direct and single-indirect), yielding `(inode_num, name, file_type_byte)`
/// for each valid entry. Deleted entries (inode_num == 0) are skipped automatically.
/// Disk I/O errors are surfaced as `Err(BlockError)` rather than panicking.
///
/// Construction is infallible; the first block is loaded lazily on the initial `next()` call.
pub(super) struct DirEntryIter {
    sb: &'static Ext2SuperBlock,
    blocks: [u32; 15],
    dir_size: u32,
    buf: Option<Arc<BlockBuf>>,
    block_size: usize,
    block_idx: usize,    // index into blocks[] of the currently loaded (or next to load) block
    block_offset: usize, // byte offset within buf; initialized to block_size as a "not loaded" sentinel
    bytes_read: u32,
}

impl DirEntryIter {
    /// Create an iterator over the directory entries of `inode`.
    ///
    /// `inode` must be a directory backed by `Ext2InodeData`; panics otherwise.
    pub(super) fn new(inode: &Inode) -> Self {
        let fs_data = inode.fs_data.downcast_ref::<Ext2InodeData>().unwrap();
        let block_size = fs_data.sb.block_size() as usize;
        Self {
            sb: fs_data.sb,
            blocks: fs_data.blocks,
            dir_size: inode.len as u32,
            buf: None,
            block_size,
            block_idx: 0,
            block_offset: block_size, // sentinel: triggers first block load on next()
            bytes_read: 0,
        }
    }
}

impl Iterator for DirEntryIter {
    type Item = Result<(u32, String, u8), BlockError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // Load the next block when the current one is exhausted (or on first call).
            if self.block_offset >= self.block_size {
                if self.bytes_read >= self.dir_size {
                    return None;
                }
                let block_ptr = match self.sb.resolve_block_ptr(&self.blocks, self.block_idx) {
                    Ok(0)    => return None,
                    Ok(ptr)  => ptr,
                    Err(e)   => return Some(Err(e)),
                };
                let sector = self.sb.block_to_sector(block_ptr);
                match self.sb.volume.as_ref().get_block(sector) {
                    Ok(block) => self.buf = Some(block),
                    Err(e)    => return Some(Err(e)),
                }
                self.block_offset = 0;
            }

            let buf    = self.buf.as_deref().unwrap();
            let offset = self.block_offset;

            if offset + DIR_ENTRY_NAME_OFFSET > self.block_size {
                // Misaligned — block is likely corrupt; skip to the next one.
                self.block_idx   += 1;
                self.block_offset = self.block_size;
                continue;
            }

            let ino      = buf.read_u32_le(offset + DIR_ENTRY_INODE_OFFSET);
            let rec_len  = buf.read_u16_le(offset + DIR_ENTRY_REC_LEN_OFFSET) as usize;
            let name_len = buf[offset + DIR_ENTRY_NAME_LEN_OFFSET] as usize;
            let ft_byte  = buf[offset + DIR_ENTRY_FILE_TYPE_OFFSET];

            if rec_len == 0 {
                // rec_len == 0 would loop forever; treat as end of block.
                self.block_idx   += 1;
                self.block_offset = self.block_size;
                continue;
            }

            self.block_offset += rec_len;
            self.bytes_read   += rec_len as u32;

            // If we've reached either a block boundary or the end of the directory
            // data, schedule the next block load (or signal exhaustion at the top).
            if self.block_offset >= self.block_size || self.bytes_read >= self.dir_size {
                self.block_idx   += 1;
                self.block_offset = self.block_size;
            }

            if ino == 0 || name_len == 0 {
                continue; // deleted or empty entry
            }

            let name_start = offset + DIR_ENTRY_NAME_OFFSET;
            let name_end   = name_start + name_len;
            if name_end > self.block_size {
                continue; // corrupt name length
            }

            match from_utf8(&buf[name_start..name_end]) {
                Ok(name) => return Some(Ok((ino, String::from(name), ft_byte))),
                Err(_)   => continue, // non-UTF-8 name; skip
            }
        }
    }
}
