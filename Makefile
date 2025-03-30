# Adjust these paths for your setup
TARGET=riscv64imac-unknown-none-elf
BUILD=target/$(TARGET)/debug
OUTPUT=kernel.elf

# Mount point where your SD card is mounted
SD_MOUNT=/mnt/sd

.PHONY: all clean copy

all: $(BUILD)/$(OUTPUT)

$(BUILD)/$(OUTPUT):
	cargo build --target $(TARGET)

copy: all
	@echo "Copying $(OUTPUT) to $(SD_MOUNT)..."
	sudo cp $(BUILD)/$(OUTPUT) $(SD_MOUNT)/
	sync
	@echo "Done."

clean:
	cargo clean

