//! Synthetic /proc filesystem.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::any::Any;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::file::{DirEntry, Error, File, FileOps, FileType, Inode, SuperBlock};
use crate::vfs::{self, FileSystem};

// =============================================================================
// Entries table
// =============================================================================

struct ProcEntry {
    name: &'static str,
    content: ProcContent,
    inode: AtomicPtr<ProcfsInode>,
}

enum ProcContent {
    Static(&'static [u8]),
    Dynamic(fn() -> String),
}

static ENTRIES: [ProcEntry; 3] = [
    ProcEntry { name: "filesystems", content: ProcContent::Dynamic(gen_filesystems), inode: AtomicPtr::new(core::ptr::null_mut()) },
    ProcEntry { name: "mounts", content: ProcContent::Dynamic(gen_mounts), inode: AtomicPtr::new(core::ptr::null_mut()) },
    ProcEntry { name: "version", content: ProcContent::Static(b"Nano OS 0.1.0 (riscv64)\n"), inode: AtomicPtr::new(core::ptr::null_mut()) },
];

fn gen_filesystems() -> String {
    use core::fmt::Write;
    let mut out = String::new();
    for fs in vfs::filesystems() {
        let prefix = if fs.requires_device() { "\t" } else { "nodev\t" };
        let _ = writeln!(out, "{}{}", prefix, fs.name());
    }
    out
}

fn gen_mounts() -> String {
    use core::fmt::Write;
    let mut out = String::new();
    for m in vfs::mounts() {
        let _ = writeln!(out, "{} {} {}", m.fs_type, m.mountpoint, m.id);
    }
    out
}

// =============================================================================
// Inode
// =============================================================================

static mut PROCFS_SB: Option<&'static dyn SuperBlock> = None;


enum ProcfsNode {
    Dir,
    File { entry: &'static ProcEntry, data: &'static [u8] },
}

struct ProcfsInode {
    node: ProcfsNode,
}

static PROCFS_DIR_OPS: ProcfsDirOps = ProcfsDirOps;
static PROCFS_FILE_OPS: ProcfsFileOps = ProcfsFileOps;

impl Inode for ProcfsInode {
    fn as_any(&self) -> &dyn Any { self }

    fn file_type(&self) -> FileType {
        match &self.node {
            ProcfsNode::Dir => FileType::Directory,
            ProcfsNode::File { .. } => FileType::RegularFile,
        }
    }

    fn len(&self) -> usize {
        match &self.node {
            ProcfsNode::Dir => 0,
            ProcfsNode::File { data, .. } => data.len(),
        }
    }

    fn file_ops(&self) -> &'static dyn FileOps {
        match &self.node {
            ProcfsNode::Dir => &PROCFS_DIR_OPS,
            ProcfsNode::File { .. } => &PROCFS_FILE_OPS,
        }
    }

    fn superblock(&self) -> Option<&'static dyn SuperBlock> {
        unsafe { *core::ptr::addr_of!(PROCFS_SB) }
    }

    fn lookup(&self, name: &str) -> Result<&'static dyn Inode, Error> {
        match &self.node {
            ProcfsNode::Dir => {
                let entry = ENTRIES.iter()
                    .find(|e| e.name == name)
                    .ok_or(Error::NotFound)?;
                let inode_ptr = entry.inode.load(Ordering::Relaxed);
                if inode_ptr.is_null() {
                    return Err(Error::NotFound);
                }
                Ok(unsafe { &*inode_ptr })
            }
            _ => Err(Error::NotADirectory),
        }
    }
}

// =============================================================================
// FileOps
// =============================================================================

struct ProcfsDirOps;

impl FileOps for ProcfsDirOps {
    fn readdir(&self, _file: &mut File) -> Result<Vec<DirEntry>, Error> {
        Ok(ENTRIES.iter().map(|e| DirEntry {
            name: String::from(e.name),
            file_type: FileType::RegularFile,
        }).collect())
    }
}

struct ProcfsFileOps;

impl FileOps for ProcfsFileOps {
    fn open(&self, inode: &'static dyn Inode) -> Result<File, Error> {
        let inode_ptr = inode as *const dyn Inode as *const ProcfsInode as *mut ProcfsInode;
        unsafe {
            if let ProcfsNode::File { entry, ref mut data } = (*inode_ptr).node {
                if let ProcContent::Dynamic(gen) = &entry.content {
                    if !data.is_empty() {
                        let old = core::slice::from_raw_parts_mut(
                            data.as_ptr() as *mut u8, data.len(),
                        );
                        let _ = Box::from_raw(old);
                    }
                    let content = gen();
                    *data = Box::leak(content.into_bytes().into_boxed_slice());
                }
            }
        }
        Ok(File::new(&PROCFS_FILE_OPS, inode))
    }

    fn read(&self, file: &mut File, buf: &mut [u8]) -> Result<usize, Error> {
        let inode = file.inode.as_any().downcast_ref::<ProcfsInode>()
            .ok_or(Error::InvalidInput)?;
        let data = match &inode.node {
            ProcfsNode::File { data, .. } => *data,
            ProcfsNode::Dir => return Err(Error::InvalidInput),
        };
        let remaining = &data[file.offset..];
        let len = remaining.len().min(buf.len());
        buf[..len].copy_from_slice(&remaining[..len]);
        file.offset += len;
        Ok(len)
    }
}

// =============================================================================
// SuperBlock
// =============================================================================

struct ProcfsSuperBlock {
    root: &'static dyn Inode,
}

impl SuperBlock for ProcfsSuperBlock {
    fn root_inode(&self) -> &'static dyn Inode { self.root }
    fn fs_type(&self) -> &'static str { "proc" }
}

// =============================================================================
// Filesystem driver
// =============================================================================

pub struct ProcfsFileSystem;

impl FileSystem for ProcfsFileSystem {
    fn name(&self) -> &'static str { "proc" }
    fn requires_device(&self) -> bool { false }
    fn mount(&self) -> Result<&'static dyn SuperBlock, Error> {
        unsafe {
            let ptr = core::ptr::addr_of!(PROCFS_SB);
            if let Some(sb) = *ptr {
                return Ok(sb);
            }
        }
        // Pre-create file inodes for each entry
        for entry in &ENTRIES {
            let data = match &entry.content {
                ProcContent::Static(data) => *data,
                ProcContent::Dynamic(_) => &[] as &'static [u8],
            };
            let inode = Box::leak(Box::new(ProcfsInode {
                node: ProcfsNode::File { entry, data },
            }));
            entry.inode.store(inode, Ordering::Relaxed);
        }

        let root = Box::leak(Box::new(ProcfsInode { node: ProcfsNode::Dir }));
        let sb: &'static dyn SuperBlock = Box::leak(Box::new(ProcfsSuperBlock { root }));
        unsafe {
            let ptr = core::ptr::addr_of_mut!(PROCFS_SB);
            *ptr = Some(sb);
        }
        Ok(sb)
    }
}

#[cfg_attr(test, allow(dead_code))]
pub static PROCFS_FS: ProcfsFileSystem = ProcfsFileSystem;
