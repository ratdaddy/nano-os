FROM ubuntu:latest
RUN apt-get update && apt-get install -y cpio && rm -rf /var/lib/apt/lists/*
RUN mkdir /work
CMD cp -r /input/* /work && \
    cd /work && \
    mkdir -p dev && \
    mkdir -p proc && \
    mknod dev/console c 5 1 && \
    mknod dev/sda  b 8 0  && \
    mknod dev/sda1 b 8 1  && \
    mknod dev/sda2 b 8 2  && \
    mkdir -p newroot && \
    mkdir -p test/dev test/emptydir && \
    mknod test/dev/tconsole c 5 1 && \
    mknod test/dev/tsda b 8 0 && \
    find * | cpio -o --format=newc > /output/initramfs.cpio
