FROM ubuntu:24.04

ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH="/opt/cargo/bin:$PATH"

RUN apt-get update && apt-get install -y \
    gdb-multiarch \
    build-essential \
    curl \
    && apt-get clean

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain none

COPY rust-toolchain.toml /tmp/
RUN cd /tmp && rustup show \
    && ln -sf $(rustup which rustc | sed 's|/bin/rustc||')/lib/rustlib/etc /opt/rust-pretty-printers

WORKDIR /workspace

ENTRYPOINT ["gdb-multiarch", "-x", "gdb/qemu-init.txt"]
