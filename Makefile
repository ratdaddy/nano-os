TARGET := riscv64imac-unknown-none-elf
BUILD_DIR := target/$(TARGET)/debug
KERNEL_ELF := $(BUILD_DIR)/kernel
KERNEL_BIN := $(BUILD_DIR)/kernel.bin
BOOT_SD := $(BUILD_DIR)/boot.sd
BOOT_ITS := $(BUILD_DIR)/boot.its

#FEATURES := --features print_dtb
FEATURES :=

SD_MOUNT=/Volumes/KERNEL

OBJCOPY := rust-objcopy

SOURCES := $(shell find src -name '*.rs') Cargo.toml Cargo.lock link.ld

.PHONY: all copy clean

all: $(BOOT_SD)

$(KERNEL_BIN): $(SOURCES)
	cargo build --target $(TARGET) $(FEATURES)
	$(OBJCOPY) --binary-architecture=riscv64 --strip-all -O binary $(KERNEL_ELF) $(KERNEL_BIN)

$(BOOT_SD): $(KERNEL_BIN) bootdata/boot.its
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

clean:
	cargo clean

monitor-cmds:
	@echo "### Commands for SOPH monitor:"
	@echo "load mmc 0:1 0x80200000 kernel.bin"
	@echo "go 0x80200000"
