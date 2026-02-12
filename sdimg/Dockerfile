FROM ubuntu:latest
RUN apt-get update && apt-get install -y dosfstools mtools parted bc exfat-fuse exfatprogs && rm -rf /var/lib/apt/lists/*

# Build an SD card image with MBR partition table and two partitions.
# Mirrors the layout of the real NanoRV SD card:
#   Partition 1: FAT32, 64 MiB, starts at sector 2048, label "KERNEL"
#   Partition 2: exFAT, 256 MiB, label "DATA"
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
    parted -s $IMG mkpart primary ntfs 65MiB 100% && \
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
    # Format partition 2 (exFAT)
    mkfs.exfat -n DATA --pack-bitmap $LOOP2 && \
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
    echo "  Partition 2: exFAT, ~256 MiB, label DATA"
