FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive
ENV RISCV_PREFIX=/opt/riscv
ENV MUSL_SYSROOT=/opt/musl-riscv64

# -----------------------------
# 🧱 Step 1: Install toolchain deps
# -----------------------------
RUN apt-get update && apt-get install -y \
    build-essential git curl ca-certificates texinfo libtool autoconf gawk \
    bison flex python3 libexpat-dev libgmp-dev libmpc-dev libmpfr-dev \
    libz-dev libisl-dev ninja-build cmake pkg-config wget \
    ccache \
  && rm -rf /var/lib/apt/lists/*

# -----------------------------
# 🧱 Step 2: Clone musl and toolchain sources (cached separately)
# -----------------------------
WORKDIR /build
RUN git clone --depth=1 https://git.musl-libc.org/musl
RUN git clone --recursive https://github.com/riscv-collab/riscv-gnu-toolchain.git

# -----------------------------
# 🧱 Step 3: Build a minimal riscv GCC to build musl
# -----------------------------
WORKDIR /build/riscv-gnu-toolchain
RUN ./configure --prefix=$RISCV_PREFIX --with-arch=rv64gc --with-abi=lp64d --disable-linux --enable-languages=c,c++
RUN make newlib -j1

# -----------------------------
# 🧱 Step 4: Build musl for riscv
# -----------------------------
WORKDIR /build/musl
RUN CC=riscv64-unknown-elf-gcc ./configure --target=riscv64 --prefix=$MUSL_SYSROOT && \
    make -j1 && \
    make install

# -----------------------------
# 🧱 Step 5: Build full riscv64-linux-musl toolchain
# -----------------------------
WORKDIR /build/riscv-gnu-toolchain
RUN make distclean && \
    ./configure \
        --prefix=$RISCV_PREFIX \
        --with-arch=rv64gc \
        --with-abi=lp64d \
        --with-sysroot=$MUSL_SYSROOT \
        --with-system-libc=$MUSL_SYSROOT \
        --with-build-sysroot=$MUSL_SYSROOT \
        --enable-multilib \
        --enable-languages=c,c++ \
    && make linux -j1

# -----------------------------
# 🦀 Install Rust nightly with components
# -----------------------------
ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:$PATH

RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain=nightly && \
    rustup component add rust-src

# Pre-install RISC-V musl target for cross-compilation
RUN rustup target add riscv64-unknown-linux-musl

# -----------------------------
# ✅ Final setup
# -----------------------------
ENV PATH="$RISCV_PREFIX/bin:$PATH"

