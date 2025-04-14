FROM ubuntu:24.04

RUN apt-get update && apt-get install -y \
    gdb-multiarch \
    build-essential \
    && apt-get clean

WORKDIR /workspace

ENTRYPOINT ["gdb-multiarch", "-x", "gdb/qemu-init.txt"]
