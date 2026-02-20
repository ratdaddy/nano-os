# devfs Block Device Resolution Brief (Option B)

This brief captures the decision for resolving `/dev/<name>` block device nodes into the correct `BlockVolume` implementation, while preserving Linux-like compatibility.

We will keep Linux-style special files (char/block inode types) and adopt a **major/minor (dev_t)**-based resolution path (Option B), with a device registry.

---

# Decision Summary

* Keep Linux-style inode types for **character** and **block** devices.
* Represent device identity using **(major, minor)** numbers.
* Resolve mount sources like `/dev/sda1` into an `Arc<dyn BlockVolume>` via a **DeviceRegistry** lookup.
* This allows creating device nodes early (e.g., in initramfs) before dynamic devfs is available.

---

# Why Option B

Option B preserves Linux-like capabilities:

* **Initramfs / early boot support**: device nodes can be created with `mknod`-like logic before devfs exists.
* **Stable identity**: device nodes can be represented by `(major, minor)` without embedding live pointers.
* **Uniform VFS semantics**: permissions, namespaces, bind mounts, and future `ioctl`/`poll` patterns remain natural.
* **Hotplug friendliness**: `/dev` can be recreated while device identity remains stable.

---

# Core Objects

## devfs inode for block devices

A devfs inode of type `InodeType::BlockDevice` stores:

* `major: u32`
* `minor: u32`

It does NOT need to store `Arc<dyn BlockVolume>`.

(An optional optimization later is to cache a weak handle, but identity remains devno-based.)

---

## DeviceRegistry

A global registry maps `dev_t` → device handles.

* `dev_t = (major, minor)`
* Values include:

  * `Arc<dyn BlockVolume>` for block devices
  * `Arc<dyn CharDevice>` (or equivalent) for char devices

This registry is populated by the device discovery/partition manager when disks/volumes are created.

---

# Resolution Flow for vfs_mount

`vfs_mount(fs_type, source_path, target_path, ...)` performs:

1. `source_inode = vfs_lookup(source_path)` (e.g. `/dev/sda1`)
2. Verify `source_inode` is `InodeType::BlockDevice`
3. Extract `(major, minor)` from the inode
4. Lookup `Arc<dyn BlockVolume>` in `DeviceRegistry`
5. `FsType::mount(volume)` returns a `SuperBlock`
6. Attach mount at `target_path`

Important:

* Filesystems (e.g. ext2) receive a `BlockVolume` handle and do not know about devfs.
* No parsing of device names like `sda1` is performed.

---

# Partition Discovery and Registration

* `BlockDriver` discovers hardware.
* `BlockDisk` owns dispatcher/queue for a physical device.
* `PartitionManager` reads partition tables using the whole-disk volume.
* `PartitionManager` creates `PartitionVolume` objects.

For each created volume:

* Allocate `(major, minor)`
* Register `(major, minor) -> Arc<dyn BlockVolume>` in `DeviceRegistry`
* If devfs exists, create a `/dev/...` node referencing that dev_t

If devfs does not yet exist (early boot):

* initramfs can still create device nodes with the known dev_t.

---

# Naming Alignment (from prior brief)

* `BlockDriver` = hardware-only driver
* `BlockDisk` = physical disk + dispatcher
* `BlockVolume` = logical address space (whole disk, partitions)
* devfs nodes identify devices via `(major, minor)`

---

# Non-Goals

* No requirement to implement full Linux `mknod` API now
* No requirement to implement sysfs/uevents now
* No write support in this phase


