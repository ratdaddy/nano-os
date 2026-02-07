TARGET := riscv64imac-unknown-none-elf
BUILD_DIR := target/$(TARGET)/debug
KERNEL_ELF := $(BUILD_DIR)/kernel
KERNEL_BIN := $(BUILD_DIR)/kernel.bin
BOOT_SD := $(BUILD_DIR)/boot.sd
BOOT_ITS := $(BUILD_DIR)/boot.its
INITRAMFS := $(BUILD_DIR)/initramfs.cpio
INITRAMFS_DIR := bootdata/initramfs
INITRAMFS_IMAGE := initramfs-builder
INIT_ELF := prog_example/target/riscv64gc-unknown-linux-musl/release/prog_example

#FEATURES := --features print_dtb
FEATURES :=

SD_MOUNT=/Volumes/KERNEL

OBJCOPY := rust-objcopy

SOURCES := $(shell find src -name '*.rs') Cargo.toml Cargo.lock link.ld

.PHONY: all copy clean gdb gdb-docker qemu-debug monitor-cmds run initramfs initramfs-docker $(INIT_ELF)

all: $(BOOT_SD)

$(KERNEL_BIN): $(SOURCES)
	cargo -Z build-std=core,alloc build --target $(TARGET) $(FEATURES)
	$(OBJCOPY) --binary-architecture=riscv64 --strip-all -O binary $(KERNEL_ELF) $(KERNEL_BIN)

$(BOOT_SD): $(KERNEL_BIN) bootdata/boot.its $(INITRAMFS)
	cp bootdata/boot.its $(BUILD_DIR)
	cp bootdata/sg2002.dtb $(BUILD_DIR)
	mkimage -f $(BOOT_ITS) $(BOOT_SD) > /dev/null 2>&1

copy: all
	@echo "Copying $(KERNEL_BIN) to $(SD_MOUNT)..."
	cp $(BOOT_SD) $(SD_MOUNT)/
	cp bootdata/fip.bin $(SD_MOUNT)/
	sync
	@diskutil eject "$$(diskutil info $(SD_MOUNT) | awk -F: '/Device Node/ {gsub(/^[ \t]+/, "", $$2); print $$2}' | sed 's/s[0-9]*$$//')"
	@echo "Done."

initramfs: $(INITRAMFS)

$(INITRAMFS): $(shell find $(INITRAMFS_DIR)) $(INIT_ELF)
	@echo "Creating initramfs.cpio..."
	mkdir -p $(BUILD_DIR)
	rm -rf $(BUILD_DIR)/initramfs
	cp -r $(INITRAMFS_DIR) $(BUILD_DIR)/initramfs
	cp $(INIT_ELF) $(BUILD_DIR)/initramfs
	mkdir -p $(BUILD_DIR)/initramfs/dev
	docker run --rm --privileged \
		-v $$(pwd)/$(BUILD_DIR)/initramfs:/input \
		-v $$(pwd)/$(BUILD_DIR):/output \
		$(INITRAMFS_IMAGE)

initramfs-docker:
	docker build -t $(INITRAMFS_IMAGE) initramfs/

$(INIT_ELF):
	make -C prog_example

run: initramfs
	cargo -Z build-std=core,alloc run --target $(TARGET) $(FEATURES)

test:
	cargo -Z build-std=core,alloc test --target $(TARGET) $(FEATURES)

lint:
	cargo clippy

format:
	cargo fmt

qemu-debug: initramfs
	cargo -Z build-std=core,alloc run -- -S -gdb tcp::1234

gdb:
	docker run --rm -it -v $$(PWD):/workspace -w /workspace riscv-gdb

gdb-docker:
	cd gdb && docker build -t riscv-gdb .

clean:
	cargo clean

monitor-cmds:
	@echo "### Commands for SOPH monitor:"
	@echo "load mmc 0:1 0x80200000 kernel.bin"
	@echo "go 0x80200000"
