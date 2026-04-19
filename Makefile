TARGET := riscv64imac-unknown-none-elf
BUILD_DIR := target/$(TARGET)/debug
KERNEL_ELF := $(BUILD_DIR)/kernel
KERNEL_BIN := $(BUILD_DIR)/kernel.bin
BOOT_SD := $(BUILD_DIR)/boot.sd
BOOT_ITS := $(BUILD_DIR)/boot.its
INITRAMFS := $(BUILD_DIR)/initramfs.cpio
INITRAMFS_DIR := bootdata/initramfs
INITRAMFS_IMAGE := initramfs-builder
SDIMG := $(BUILD_DIR)/sd.img
SDIMG_IMAGE := sdimg-builder
EXT2IMG := $(BUILD_DIR)/ext2.img
EXT2IMG_IMAGE := ext2img-builder
INIT_ELF := prog_example/target/riscv64gc-unknown-linux-musl/release/prog_example

#FEATURES := --features print_dtb,trace_syscalls,trace_trap,trace_scheduler,trace_process,trace_volumes
FEATURES :=

SD_MOUNT=/Volumes/KERNEL

OBJCOPY := rust-objcopy

SOURCES := $(shell find src -name '*.rs') Cargo.toml Cargo.lock link.ld

LOG := /tmp/nano-os.log
LOG_QEMU := /tmp/nano-os-qemu.log
BLUE := \033[38;5;39m
RESET := \033[0m

define log-header
@tmux list-panes -a -F '#{pane_id} #{pane_title}' | awk '$$2=="log"{print $$1}' | xargs -I{} tmux send-keys -t {} q 2>/dev/null || true
@printf '\n\n$(BLUE)▶ $(1) · %s$(RESET)\n\n' "$$(date '+%Y-%m-%d %H:%M:%S')" | tee -a $(LOG)
endef

.PHONY: build all copy clean tail-log gdb gdb-docker qemu-debug monitor-cmds run initramfs initramfs-docker sdimg sdimg-docker ext2img ext2img-docker $(INIT_ELF)

build:
	$(call log-header,make build)
	cargo -Z build-std=core,alloc build --target $(TARGET) $(FEATURES) --color always 2>&1 | tee -a $(LOG)

tail-log:
	@tmux select-pane -T log 2>/dev/null || true
	tail -F $(LOG)

all: $(BOOT_SD)

$(KERNEL_BIN): $(SOURCES)
	cargo -Z build-std=core,alloc build --target $(TARGET) $(FEATURES)
	$(OBJCOPY) --binary-architecture=riscv64 --strip-all -O binary $(KERNEL_ELF) $(KERNEL_BIN)

$(BOOT_SD): $(KERNEL_BIN) bootdata/boot.its $(INITRAMFS)
	cp bootdata/boot.its $(BUILD_DIR)
	cp bootdata/sg2002.dtb $(BUILD_DIR)
	mkimage -f $(BOOT_ITS) $(BOOT_SD) > /dev/null 2>&1

copy: all $(EXT2IMG)
	@echo "Copying files to SD card..."
	@# Copy to FAT32 partition (partition 1)
	@echo "  - Copying boot files to $(SD_MOUNT)..."
	cp $(BOOT_SD) $(SD_MOUNT)/
	cp bootdata/fip.bin $(SD_MOUNT)/
	sync
	@# Get the raw disk device (e.g., /dev/disk4 from /dev/disk4s1)
	@DISK_DEV=$$(diskutil info $(SD_MOUNT) | awk -F: '/Device Node/ {gsub(/^[ \t]+/, "", $$2); print $$2}' | sed 's/s[0-9]*$$//') && \
	echo "  - Unmounting all partitions..." && \
	diskutil unmountDisk $${DISK_DEV} 2>/dev/null || true && \
	echo "  - Zeroing sector 2000 (write demo verification)..." && \
	sudo dd if=/dev/zero of=$${DISK_DEV} bs=512 count=1 seek=2000 conv=notrunc 2>/dev/null && \
	echo "  - Writing ext2 filesystem to $${DISK_DEV}s2 (partition 2)..." && \
	sudo dd if=$(EXT2IMG) of=$${DISK_DEV}s2 bs=1048576 && \
	sync && \
	diskutil eject $$DISK_DEV
	@echo "Done. SD card ejected."

initramfs: $(INITRAMFS)

$(INITRAMFS): $(shell find $(INITRAMFS_DIR)) $(INIT_ELF)
	@echo "Creating initramfs.cpio..."
	mkdir -p $(BUILD_DIR)
	rm -rf $(BUILD_DIR)/initramfs
	cp -r $(INITRAMFS_DIR) $(BUILD_DIR)/initramfs
	cp $(INIT_ELF) $(BUILD_DIR)/initramfs
	# mkdir -p $(BUILD_DIR)/initramfs/dev
	docker run --rm --privileged \
		-v $$(pwd)/$(BUILD_DIR)/initramfs:/input \
		-v $$(pwd)/$(BUILD_DIR):/output \
		$(INITRAMFS_IMAGE)

initramfs-docker:
	docker build -t $(INITRAMFS_IMAGE) initramfs/

sdimg: $(SDIMG)

$(SDIMG): $(BOOT_SD) sdimg/Dockerfile
	@echo "Creating sd.img..."
	@mkdir -p $(BUILD_DIR)
	@mkdir -p $(BUILD_DIR)/sdimg_input
	@touch $(BUILD_DIR)/sd.img
	@cp $(BOOT_SD) $(BUILD_DIR)/sdimg_input/boot.sd
	@cp bootdata/fip.bin $(BUILD_DIR)/sdimg_input/fip.bin
	@docker run --rm --privileged \
		-v $$(pwd)/$(BUILD_DIR)/sdimg_input:/input \
		-v $$(pwd)/$(BUILD_DIR):/output \
		$(SDIMG_IMAGE)
	@rm -rf $(BUILD_DIR)/sdimg_input
	@echo "Done."

sdimg-docker:
	docker build -t $(SDIMG_IMAGE) sdimg/

ext2img: $(EXT2IMG)

$(EXT2IMG): ext2img/Dockerfile
	@echo "Creating ext2.img..."
	@mkdir -p $(BUILD_DIR)
	@docker run --rm --privileged \
		-v $$(pwd)/$(BUILD_DIR):/output \
		$(EXT2IMG_IMAGE)
	@echo "Done."

ext2img-docker:
	docker build -t $(EXT2IMG_IMAGE) ext2img/

$(INIT_ELF):
	make -C prog_example

run: initramfs $(SDIMG)
	dd if=/dev/zero of=$(SDIMG) bs=512 count=1 seek=2000 conv=notrunc 2>/dev/null
	script -q $(LOG_QEMU) cargo -Z build-std=core,alloc run --target $(TARGET) $(FEATURES)

test: initramfs
	$(call log-header,make test)
	@test -f $(SDIMG) || { mkdir -p $(BUILD_DIR) && touch -t 197001010000 $(SDIMG); }
	cargo -Z build-std=core,alloc test --target $(TARGET) $(FEATURES) --color always 2>&1 | tee -a $(LOG)

lint:
	cargo clippy

format:
	cargo fmt

qemu-debug: initramfs $(SDIMG)
	cargo -Z build-std=core,alloc run -- -S -gdb tcp::1234

gdb:
	docker run --rm -it -v $$(PWD):/workspace -w /workspace riscv-gdb

gdb-docker:
	docker build -t riscv-gdb -f gdb/Dockerfile .

clean:
	cargo clean

monitor-cmds:
	@echo "### Commands for SOPH monitor:"
	@echo "load mmc 0:1 0x80200000 kernel.bin"
	@echo "go 0x80200000"
