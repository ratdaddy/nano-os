INIT_ELF := target/riscv64gc-unknown-linux-musl/release/prog_example

.PHONY: all build clean docker-build docker-run

all: $(INIT_ELF)

$(INIT_ELF): src/main.rs
	docker run --rm -v "$$(pwd)":/work -w /work rust-riscv-musl-static \
		cargo +nightly build --release --target riscv64gc-unknown-linux-musl -Z build-std=std,panic_abort

build:
	cargo +nightly build --release --target riscv64gc-unknown-linux-musl -Z build-std=std,panic_abort

clean:
	cargo clean

docker-build:
	docker build -t rust-riscv-musl-static .

docker-run:
	docker run -it --rm -v "$$(pwd)":/work -w /work rust-riscv-musl-static bash
