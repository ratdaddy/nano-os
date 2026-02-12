# SD Card Partitioning on macOS (MBR + FAT32 + ext RootFS)

This document describes how to partition a physical SD card on macOS for nano‑os development.

Layout:

* Partition 1: `KERNEL` (FAT32) – boot files (`fip.bin`, `boot.sd`, dtbs, etc.)
* Partition 2: `ROOTFS` (ext2/ext4 written via `dd`)
* Partition table: MBR (DOS)

---

## Step 1 — Identify the SD Card

List disks:

```bash
diskutil list
```

Find your SD card (example):

```
/dev/disk6 (external, physical)
```

⚠️ Be absolutely certain this is your SD card before continuing.

---

## Step 2 — Create MBR + Two Partitions

macOS cannot create ext4 directly, so we create a placeholder filesystem (ExFAT) for partition 2, which we will overwrite later.

```bash
diskutil partitionDisk /dev/disk6 MBR \
  FAT32 KERNEL 256M \
  ExFAT ROOTFS 0B
```

This creates:

* `/dev/disk6s1` → FAT32 (KERNEL)
* `/dev/disk6s2` → ExFAT (ROOTFS placeholder)

---

## Step 3 — Verify Partition Layout

```bash
diskutil list /dev/disk6
```

Expected output (example):

```
/dev/disk6 (external, physical):
   #:                       TYPE NAME       SIZE
   0:     FDisk_partition_scheme            *32.0 GB
   1:             DOS_FAT_32 KERNEL         256.0 MB
   2:                      ExFAT ROOTFS     31.7 GB
```

The MBR signature (0x55AA) is written automatically.

---

## Step 4 — Write ext2/ext4 Root Filesystem

Unmount the disk first:

```bash
diskutil unmountDisk /dev/disk6
```

Then overwrite partition 2 with your ext filesystem image:

```bash
sudo dd if=rootfs.ext4 of=/dev/rdisk6s2 bs=4m conv=sync status=progress
```

Notes:

* Use `rdisk` for faster raw writes.
* This replaces the ExFAT filesystem entirely.
* The MBR partition table remains intact.

---

## Step 5 — Ignore macOS Warnings

After writing ext4, macOS may display:

> “The disk you inserted was not readable by this computer”

This is expected. The partition now contains ext4.

Click **Ignore**.

---

## Important Clarifications

### MBR Partition Type

The MBR partition type may still indicate ExFAT (0x07) even though the contents are ext4.

This does not matter.

Your kernel should:

1. Parse MBR
2. Use partition start + size
3. Probe filesystem superblock (ext magic = 0xEF53)
4. Mount based on actual on-disk data

Never rely on the MBR "type" byte to determine filesystem type.

---

## Optional: Making the Type "Correct"

If you later create the image from Linux using `parted` or `fdisk`, the partition type can be set to:

```
0x83 (Linux)
```

This is cosmetic only.

---

## Recommended Long-Term Workflow

For safer iteration:

1. Build a full `sd.img` with MBR + partitions using Linux/Docker.
2. Test in QEMU.
3. Write the entire image to the SD card when needed.

This avoids repeated live partition edits and reduces risk of writing to the wrong disk.

