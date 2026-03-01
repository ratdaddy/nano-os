FROM ubuntu:latest
RUN apt-get update && apt-get install -y e2fsprogs && rm -rf /var/lib/apt/lists/*

# Create an ext2 filesystem image for the second partition
# Small size - just enough for test files (will be written to partition 2)
# Output: ext2.img written to /output
CMD set -e && \
    IMG=/output/ext2.img && \
    IMG_SIZE=4M && \
    \
    # Create empty image file
    truncate -s $IMG_SIZE $IMG && \
    \
    # Format as ext2 with label "BOOT"
    mkfs.ext2 -L BOOT $IMG && \
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
    \
    # Unmount
    umount /mnt/ext2 && \
    \
    echo "Created $IMG ($(stat -c%s $IMG) bytes)" && \
    echo "  Filesystem: ext2, label BOOT" && \
    echo "    - /hello.txt" && \
    echo "    - /subdir/test.txt" && \
    echo "    - /console (c 5,1)" && \
    echo "    - /sda (b 8,0)"
