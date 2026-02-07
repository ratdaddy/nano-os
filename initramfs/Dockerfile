FROM ubuntu:latest
RUN apt-get update && apt-get install -y cpio && rm -rf /var/lib/apt/lists/*
RUN mkdir /work
CMD cp -r /input/* /work && \
    cd /work && \
    mkdir -p dev && \
    mknod dev/console c 5 1 && \
    mkdir -p newroot && \
    find * | cpio -o --format=newc > /output/initramfs.cpio
