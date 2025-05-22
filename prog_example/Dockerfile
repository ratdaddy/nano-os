FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive
ENV RUSTUP_HOME=/opt/rustup
ENV CARGO_HOME=/opt/cargo
ENV PATH="$CARGO_HOME/bin:$PATH"
ENV MUSL_TOOLCHAIN_DIR=/opt/musl-toolchain

# Install build dependencies
RUN apt-get update && apt-get install -y \
    curl build-essential git ca-certificates \
    gcc g++ file xz-utils \
    && rm -rf /var/lib/apt/lists/*

# Download and install musl cross toolchain
RUN curl -L https://musl.cc/riscv64-linux-musl-cross.tgz | tar -xz -C /opt && \
    mv /opt/riscv64-linux-musl-cross $MUSL_TOOLCHAIN_DIR

# Install Rust + nightly + rust-src
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain nightly && \
    rustup component add rust-src

# Preinstall the musl target
RUN rustup target add riscv64gc-unknown-linux-musl

# Install important things
RUN apt update -y && apt install -y vim
