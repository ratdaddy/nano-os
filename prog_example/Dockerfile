FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive
ENV RISCV_PREFIX=/opt/riscv

# Install dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    git \
    curl \
    ca-certificates \
    texinfo \
    libtool \
    autoconf \
    gawk \
    bison \
    flex \
    python3 \
    libexpat-dev \
    libgmp-dev \
    libmpc-dev \
    libmpfr-dev \
    libz-dev \
    libisl-dev \
    ninja-build \
    cmake \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Clone the riscv-gnu-toolchain repo with submodules
RUN git clone --recursive https://github.com/riscv-collab/riscv-gnu-toolchain.git /build/riscv-gnu-toolchain

# Build the musl toolchain
WORKDIR /build/riscv-gnu-toolchain
RUN ./configure --prefix=$RISCV_PREFIX --with-arch=rv64gc --with-abi=lp64d --enable-multilib --with-system-libc=musl
RUN make linux -j$(nproc)

# Add to PATH
ENV PATH="$RISCV_PREFIX/bin:$PATH"
