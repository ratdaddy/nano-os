//! Synthetic /proc filesystem.

use core::fmt::Write;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::dev;
use crate::file::{DirEntry, Error, File, FileOps, FileType, Inode, InodeOps, SuperBlock};
use crate::vfs::{self, FileSystem};

// =============================================================================
// Entries table
// =============================================================================

struct ProcEntry {
    name: &'static str,
    content: ProcContent,
}

enum ProcContent {
    Static(&'static [u8]),
    Dynamic(fn() -> String),
}

static ENTRIES: [ProcEntry; 4] = [
    ProcEntry { name: "devices",     content: ProcContent::Dynamic(gen_devices) },
    ProcEntry { name: "filesystems", content: ProcContent::Dynamic(gen_filesystems) },
    ProcEntry { name: "mounts",      content: ProcContent::Dynamic(gen_mounts) },
    ProcEntry { name: "version",     content: ProcContent::Static(b"Nano OS 0.1.0 (riscv64)\n") },
];

fn gen_devices() -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Character devices:");
    dev::chrdev_for_each(|major, minor, name| {
        let _ = writeln!(out, "{:3} {} ({}:{})", major, name, major, minor);
    });
    let _ = writeln!(out);
    let _ = writeln!(out, "Block devices:");
    dev::blkdev_for_each(|major, minor, name| {
        let _ = writeln!(out, "{:3} {} ({}:{})", major, name, major, minor);
    });
    out
}

fn gen_filesystems() -> String {
    let mut out = String::new();
    for fs in vfs::filesystems() {
        let prefix = if fs.nodev() { "nodev\t" } else { "\t" };
        let _ = writeln!(out, "{}{}", prefix, fs.name());
    }
    out
}

fn gen_mounts() -> String {
    let mut out = String::new();
    for m in vfs::mounts() {
        let _ = writeln!(out, "{} {} {}", m.fs_type, m.mountpoint, m.id);
    }
    out
}

// =============================================================================
// Node types
// =============================================================================

enum ProcfsNode {
    Dir,
    File { entry: &'static ProcEntry },
}

/// Content buffer for an open procfs file. Freed when the File is closed.
struct ProcfsFileData(Box<[u8]>);

// =============================================================================
// Ops tables
// =============================================================================

struct ProcfsInodeOps;
struct ProcfsDirOps;
struct ProcfsFileOps;

static PROCFS_INODE_OPS: ProcfsInodeOps = ProcfsInodeOps;
static PROCFS_DIR_OPS: ProcfsDirOps = ProcfsDirOps;
static PROCFS_FILE_OPS: ProcfsFileOps = ProcfsFileOps;

impl InodeOps for ProcfsInodeOps {
    fn lookup(&self, inode: &Arc<Inode>, name: &str) -> Result<Arc<Inode>, Error> {
        let node = inode.fs_data.downcast_ref::<ProcfsNode>().unwrap();
        match node {
            ProcfsNode::Dir => {
                let (idx, entry) = ENTRIES.iter().enumerate()
                    .find(|(_, e)| e.name == name)
                    .ok_or(Error::NotFound)?;
                Ok(Arc::new(Inode {
                    ino: idx as u64 + 2,
                    file_type: FileType::RegularFile,
                    len: 0,
                    iops: &PROCFS_INODE_OPS,
                    fops: &PROCFS_FILE_OPS,
                    sb: None,
                    rdev: None,
                    fs_data: Box::new(ProcfsNode::File { entry }),
                }))
            }
            ProcfsNode::File { .. } => Err(Error::NotADirectory),
        }
    }
}

impl FileOps for ProcfsDirOps {
    fn readdir(&self, file: &mut File) -> Result<Vec<DirEntry>, Error> {
        match file.inode.fs_data.downcast_ref::<ProcfsNode>().unwrap() {
            ProcfsNode::Dir => Ok(ENTRIES.iter().map(|e| DirEntry {
                name: String::from(e.name),
                file_type: FileType::RegularFile,
            }).collect()),
            ProcfsNode::File { .. } => Err(Error::NotADirectory),
        }
    }
}

impl FileOps for ProcfsFileOps {
    /// Generate file content at open time and attach it to a per-open inode.
    /// The content buffer is freed when the File is dropped.
    fn open(&self, inode: Arc<Inode>) -> Result<File, Error> {
        let node = inode.fs_data.downcast_ref::<ProcfsNode>().ok_or(Error::InvalidInput)?;
        let data: Box<[u8]> = match node {
            ProcfsNode::File { entry } => match &entry.content {
                ProcContent::Static(bytes) => bytes.to_vec().into_boxed_slice(),
                ProcContent::Dynamic(gen) => gen().into_bytes().into_boxed_slice(),
            },
            ProcfsNode::Dir => return Err(Error::InvalidInput),
        };
        let len = data.len();
        let open_inode = Arc::new(Inode {
            ino: inode.ino,
            file_type: FileType::RegularFile,
            len,
            iops: &PROCFS_INODE_OPS,
            fops: &PROCFS_FILE_OPS,
            sb: None,
            rdev: None,
            fs_data: Box::new(ProcfsFileData(data)),
        });
        Ok(File::new(&PROCFS_FILE_OPS, open_inode))
    }

    fn read(&self, file: &mut File, buf: &mut [u8]) -> Result<usize, Error> {
        let data = file.inode.fs_data.downcast_ref::<ProcfsFileData>().ok_or(Error::InvalidInput)?;
        let remaining = &data.0[file.offset..];
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
    root: Arc<Inode>,
}

impl SuperBlock for ProcfsSuperBlock {
    fn root_inode(&self) -> Arc<Inode> { Arc::clone(&self.root) }
    fn fs_type(&self) -> &'static str { "proc" }
}

// =============================================================================
// Filesystem driver
// =============================================================================

pub struct ProcfsFileSystem;

impl FileSystem for ProcfsFileSystem {
    fn name(&self) -> &'static str { "proc" }
    fn nodev(&self) -> bool { true }
    fn mount(&self, _source: Option<&str>) -> Result<&'static dyn SuperBlock, Error> {
        let root = Arc::new(Inode {
            ino: 1,
            file_type: FileType::Directory,
            len: 0,
            iops: &PROCFS_INODE_OPS,
            fops: &PROCFS_DIR_OPS,
            sb: None,
            rdev: None,
            fs_data: Box::new(ProcfsNode::Dir),
        });
        Ok(Box::leak(Box::new(ProcfsSuperBlock { root })))
    }
}

pub static PROCFS_FS: ProcfsFileSystem = ProcfsFileSystem;

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;
    use alloc::sync::Arc;
    use super::*;

    fn make_root() -> Arc<Inode> {
        Arc::new(Inode {
            ino: 1,
            file_type: FileType::Directory,
            len: 0,
            iops: &PROCFS_INODE_OPS,
            fops: &PROCFS_DIR_OPS,
            sb: None,
            rdev: None,
            fs_data: Box::new(ProcfsNode::Dir),
        })
    }

    // -- ino tests --

    #[test_case]
    fn test_root_ino() {
        println!("Testing procfs root inode has ino=1 and is a directory...");

        let root = make_root();
        assert_eq!(root.ino, 1);
        assert_eq!(root.file_type, FileType::Directory);
    }

    #[test_case]
    fn test_lookup_inos() {
        println!("Testing procfs lookup assigns ino = entry_index + 2...");

        // ENTRIES order: [0]="devices", [1]="filesystems", [2]="mounts", [3]="version"
        let root = make_root();
        assert_eq!(root.iops.lookup(&root, "devices").unwrap().ino,     2);
        assert_eq!(root.iops.lookup(&root, "filesystems").unwrap().ino, 3);
        assert_eq!(root.iops.lookup(&root, "mounts").unwrap().ino,      4);
        assert_eq!(root.iops.lookup(&root, "version").unwrap().ino,     5);
    }

    // -- lookup tests --

    #[test_case]
    fn test_lookup_returns_regular_file() {
        println!("Testing procfs lookup returns RegularFile...");

        let root = make_root();
        let inode = root.iops.lookup(&root, "version").unwrap();
        assert_eq!(inode.file_type, FileType::RegularFile);
    }

    #[test_case]
    fn test_lookup_not_found() {
        println!("Testing procfs lookup returns NotFound for unknown name...");

        let root = make_root();
        let result = root.iops.lookup(&root, "nonexistent");
        assert!(matches!(result, Err(Error::NotFound)));
    }

    #[test_case]
    fn test_lookup_on_file_returns_not_a_directory() {
        println!("Testing procfs lookup on file inode returns NotADirectory...");

        let root = make_root();
        let file_inode = root.iops.lookup(&root, "version").unwrap();
        let result = file_inode.iops.lookup(&file_inode, "child");
        assert!(matches!(result, Err(Error::NotADirectory)));
    }

    // -- readdir tests --

    #[test_case]
    fn test_readdir_returns_all_entries() {
        println!("Testing procfs readdir returns all entries as RegularFile...");

        let root = make_root();
        let mut file = root.fops.open(Arc::clone(&root)).unwrap();
        let entries = root.fops.readdir(&mut file).unwrap();

        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].name, "devices");
        assert_eq!(entries[0].file_type, FileType::RegularFile);
        assert_eq!(entries[1].name, "filesystems");
        assert_eq!(entries[1].file_type, FileType::RegularFile);
        assert_eq!(entries[2].name, "mounts");
        assert_eq!(entries[2].file_type, FileType::RegularFile);
        assert_eq!(entries[3].name, "version");
        assert_eq!(entries[3].file_type, FileType::RegularFile);
    }

    // -- open + read tests --

    #[test_case]
    fn test_static_content_read() {
        println!("Testing procfs static file content reads correctly...");

        let root = make_root();
        let inode = root.iops.lookup(&root, "version").unwrap();
        let fops = inode.fops;
        let mut file = fops.open(Arc::clone(&inode)).unwrap();

        let mut buf = [0u8; 64];
        let n = fops.read(&mut file, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"Nano OS 0.1.0 (riscv64)\n");
    }

    #[test_case]
    fn test_open_sets_len_to_content_size() {
        println!("Testing procfs open sets inode len to generated content size...");

        let root = make_root();
        let inode = root.iops.lookup(&root, "version").unwrap();
        let fops = inode.fops;
        let file = fops.open(Arc::clone(&inode)).unwrap();

        let expected_len = b"Nano OS 0.1.0 (riscv64)\n".len();
        assert_eq!(file.inode.len, expected_len);
    }

    #[test_case]
    fn test_open_preserves_ino() {
        println!("Testing procfs per-open inode preserves lookup ino...");

        let root = make_root();
        let lookup_inode = root.iops.lookup(&root, "version").unwrap();
        let lookup_ino = lookup_inode.ino;
        let file = lookup_inode.fops.open(Arc::clone(&lookup_inode)).unwrap();
        assert_eq!(file.inode.ino, lookup_ino);
    }
}
