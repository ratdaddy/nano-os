FROM ubuntu:latest
RUN apt-get update && apt-get install -y e2fsprogs && rm -rf /var/lib/apt/lists/*

# Create an ext3 filesystem image for the second partition
# Small size - just enough for test files (will be written to partition 2)
# Output: ext2.img written to /output
CMD set -e && \
    IMG=/output/ext2.img && \
    IMG_SIZE=64M && \
    \
    # Create empty image file
    truncate -s $IMG_SIZE $IMG && \
    \
    # Format as ext3 with label "BOOT"
    mkfs.ext3 -L BOOT $IMG && \
    \
    # Mount and add test files
    mkdir -p /mnt/ext2 && \
    mount -o loop $IMG /mnt/ext2 && \
    \
    # Create test files
    echo "Hello from ext2 filesystem on LicheeRV Nano!" > /mnt/ext2/hello.txt && \
    mkdir -p /mnt/ext2/subdir && \
    echo "File in subdirectory" > /mnt/ext2/subdir/test.txt && \
    mknod /mnt/ext2/console c 5 1 && \
    mknod /mnt/ext2/sda b 8 0 && \
    dd if=/dev/zero bs=4096 count=12 2>/dev/null | tr '\0' 'A' > /mnt/ext2/large.txt && \
    printf 'Hello from indirect block!\n' >> /mnt/ext2/large.txt && \
    \
    # Unmount
    umount /mnt/ext2 && \
    \
    echo "Created $IMG ($(stat -c%s $IMG) bytes)" && \
    echo "  Filesystem: ext3, label BOOT" && \
    echo "    - /hello.txt" && \
    echo "    - /subdir/test.txt" && \
    echo "    - /console (c 5,1)" && \
    echo "    - /sda (b 8,0)" && \
    echo "    - /large.txt (48 KiB padding + indirect block marker)"
