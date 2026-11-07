TARGET  := aarch64-unknown-none
PROFILE := release
# O mesmo diretório que o cargo usa. Sem isto, `rust-objcopy` lê um ELF velho
# quando CARGO_TARGET_DIR aponta para fora de `target/`.
CARGO_TARGET_DIR ?= target
ELF     := $(CARGO_TARGET_DIR)/$(TARGET)/$(PROFILE)/rasppios
KERNEL  := boot/kernel8.img
IMG     := rasppios.img

.PHONY: all build kernel run check firmware image write clean

all: kernel

build:
	cargo build --$(PROFILE)

kernel: build
	rust-objcopy -O binary $(ELF) $(KERNEL)
	@ls -l $(KERNEL)

run: kernel
	bash scripts/qemu.sh $(ELF)

check: kernel
	bash scripts/check-qemu.sh

firmware:
	bash scripts/fetch-firmware.sh

image: kernel firmware
	bash scripts/image.sh $(IMG)

# Copia para o exFAT já montado. Ex.: make write DEST=/media/vladimir/PENDRIVE
write: kernel firmware
	@test -n "$(DEST)" || { echo "uso: make write DEST=/media/vladimir/PENDRIVE"; exit 1; }
	bash scripts/install-boot.sh $(DEST)

clean:
	cargo clean
	rm -f $(KERNEL) $(IMG)
