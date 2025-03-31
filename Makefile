TARGET := riscv64imac-unknown-none-elf
BUILD_DIR := target/$(TARGET)/debug
KERNEL_ELF := $(BUILD_DIR)/kernel
KERNEL_BIN := $(BUILD_DIR)/kernel.bin

SD_MOUNT=/Volumes/KERNEL

OBJCOPY := rust-objcopy

SOURCES := $(shell find src -name '*.rs') Cargo.toml Cargo.lock link.ld

.PHONY: all copy clean

all: $(KERNEL_BIN)

$(KERNEL_BIN): $(SOURCES)
	cargo build --target $(TARGET)
	$(OBJCOPY) --binary-architecture=riscv64 --strip-all -O binary $(KERNEL_ELF) $(KERNEL_BIN)

copy: all
	@echo "Copying $(KERNEL_BIN) to $(SD_MOUNT)..."
	cp $(KERNEL_BIN) $(SD_MOUNT)/
	sync
	@diskutil eject "$$(diskutil info $(SD_MOUNT) | awk -F: '/Device Node/ {gsub(/^[ \t]+/, "", $$2); print $$2}' | sed 's/s[0-9]*$$//')"
	@echo "Done."

clean:
	cargo clean

monitor-cmds:
	@echo "### Commands for SOPH monitor:"
	@echo "load mmc 0:1 0x80200000 kernel.bin"
	@echo "go 0x80200000"
