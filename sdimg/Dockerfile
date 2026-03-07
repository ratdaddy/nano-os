FROM ubuntu:latest
RUN apt-get update && apt-get install -y dosfstools mtools parted bc e2fsprogs && rm -rf /var/lib/apt/lists/*

# Build an SD card image with MBR partition table and two partitions.
#   Partition 1: FAT32, 64 MiB, starts at sector 2048, label "KERNEL"
#   Partition 2: ext2, 256 MiB, label "BOOT" (with test files: hello.txt, subdir/test.txt)
#
# Input:  files to copy mounted at /input (optional)
# Output: sd.img written to /output
CMD set -e && \
    IMG=/output/sd.img && \
    IMG_SIZE=384M && \
    \
    # Create raw image
    truncate -s $IMG_SIZE $IMG && \
    \
    # Create MBR partition table with two partitions starting at 1MiB (sector 2048)
    parted -s $IMG mklabel msdos && \
    parted -s $IMG mkpart primary fat32 1MiB 65MiB && \
    parted -s $IMG set 1 lba on && \
    parted -s $IMG mkpart primary ext2 65MiB 100% && \
    \
    # Set up loop device for the whole image
    LOOP=$(losetup --show -f $IMG) && \
    \
    # Format partition 1 (FAT32) using offset (mkfs.vfat supports this)
    mkfs.vfat -F 32 -n KERNEL -s 8 --offset 2048 $LOOP && \
    \
    # Create a second loop device for partition 2 with explicit offset and size
    PART2_OFFSET=$((65 * 1024 * 1024)) && \
    IMG_SIZE=$(stat -c%s $IMG) && \
    PART2_SIZE=$((IMG_SIZE - PART2_OFFSET)) && \
    LOOP2=$(losetup --show -f --offset $PART2_OFFSET --sizelimit $PART2_SIZE $IMG) && \
    \
    # Format partition 2 (ext2)
    mkfs.ext2 -L BOOT $LOOP2 && \
    \
    # Mount ext2 partition and add test files
    mkdir -p /mnt/ext2 && \
    mount $LOOP2 /mnt/ext2 && \
    echo "Hello from ext2 filesystem!" > /mnt/ext2/hello.txt && \
    mkdir -p /mnt/ext2/subdir && \
    echo "File in subdirectory" > /mnt/ext2/subdir/test.txt && \
    mknod /mnt/ext2/console c 5 1 && \
    mknod /mnt/ext2/sda b 8 0 && \
    dd if=/dev/zero bs=4096 count=12 2>/dev/null | tr '\0' 'A' > /mnt/ext2/large.txt && \
    printf 'Hello from indirect block!\n' >> /mnt/ext2/large.txt && \
    umount /mnt/ext2 && \
    \
    # Clean up second loop device
    losetup -d $LOOP2 && \
    \
    # Copy input files if any exist (using mtools to avoid mounting)
    PART1_OFFSET=$((2048 * 512)) && \
    if [ -d /input ] && [ "$(ls -A /input 2>/dev/null)" ]; then \
        for f in /input/*; do \
            mcopy -i "$IMG@@$PART1_OFFSET" "$f" ::/; \
        done; \
    fi && \
    \
    losetup -d $LOOP && \
    echo "Created $IMG ($(stat -c%s $IMG) bytes)" && \
    echo "  Partition 1: FAT32, 64 MiB, label KERNEL" && \
    echo "  Partition 2: ext2, ~256 MiB, label BOOT" && \
    echo "    - /hello.txt" && \
    echo "    - /subdir/test.txt" && \
    echo "    - /console (c 5,1)" && \
    echo "    - /sda (b 8,0)" && \
    echo "    - /large.txt (48 KiB padding + indirect block marker)"
