//! ext2 InodeOps and FileOps implementations

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::file::{DirEntry, Error, File, FileOps, FileType, Inode, InodeOps};

use super::dir::{DirEntryIter, EXT2_FT_BLKDEV, EXT2_FT_CHRDEV, EXT2_FT_DIR, EXT2_FT_REG_FILE};
use super::inode::Ext2InodeData;
use super::superblock::NDIR_BLOCKS;

pub(super) struct Ext2InodeOps;
pub(super) struct Ext2FileOps;

pub(super) static EXT2_INODE_OPS: Ext2InodeOps = Ext2InodeOps;
pub(super) static EXT2_FILE_OPS: Ext2FileOps = Ext2FileOps;

impl InodeOps for Ext2InodeOps {
    fn lookup(&self, inode: &Arc<Inode>, name: &str) -> Result<Arc<Inode>, Error> {
        if inode.file_type != FileType::Directory {
            return Err(Error::NotADirectory);
        }

        let fs_data = inode.fs_data.downcast_ref::<Ext2InodeData>().unwrap();

        for entry in DirEntryIter::new(inode) {
            let (ino, entry_name, _) = entry.map_err(|_| Error::NotFound)?;
            if entry_name == name {
                return fs_data.sb.get_or_read_inode(ino).map_err(|_| Error::NotFound);
            }
        }

        Err(Error::NotFound)
    }
}

impl FileOps for Ext2FileOps {
    fn read(&self, file: &mut File, buf: &mut [u8]) -> Result<usize, Error> {
        if file.inode.file_type != FileType::RegularFile {
            return Err(Error::InvalidInput);
        }

        let file_len = file.inode.len;
        if file.offset >= file_len || buf.is_empty() {
            return Ok(0);
        }

        let fs_data = file.inode.fs_data.downcast_ref::<Ext2InodeData>().unwrap();
        let sb = fs_data.sb;
        let block_size = sb.block_size() as usize;
        let mut buf_pos = 0;

        while buf_pos < buf.len() && file.offset < file_len {
            let block_idx = file.offset / block_size;
            if block_idx >= NDIR_BLOCKS {
                break; // indirect blocks not yet supported
            }
            let block_ptr = fs_data.blocks[block_idx];
            if block_ptr == 0 {
                break; // sparse hole or end of allocated blocks
            }

            let block_buf = sb.volume.as_ref()
                .get_block(sb.block_to_sector(block_ptr))
                .map_err(|_| Error::InvalidInput)?;

            let block_offset  = file.offset % block_size;
            let copy_len = (block_size - block_offset)
                .min(file_len - file.offset)
                .min(buf.len() - buf_pos);

            buf[buf_pos..buf_pos + copy_len]
                .copy_from_slice(&block_buf[block_offset..block_offset + copy_len]);

            file.offset += copy_len;
            buf_pos     += copy_len;
            // block_buf dropped here — cache slot unpinned
        }

        Ok(buf_pos)
    }

    fn readdir(&self, file: &mut File) -> Result<Vec<DirEntry>, Error> {
        if file.inode.file_type != FileType::Directory {
            return Err(Error::NotADirectory);
        }

        DirEntryIter::new(&file.inode)
            .map(|r| {
                let (_, name, ft_byte) = r.map_err(|_| Error::InvalidInput)?;
                let file_type = match ft_byte {
                    EXT2_FT_REG_FILE => FileType::RegularFile,
                    EXT2_FT_DIR      => FileType::Directory,
                    EXT2_FT_CHRDEV   => FileType::CharDevice,
                    EXT2_FT_BLKDEV   => FileType::BlockDevice,
                    _                => FileType::RegularFile,
                };
                Ok(DirEntry { name, file_type })
            })
            .collect()
    }
}
