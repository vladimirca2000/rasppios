TARGET  := aarch64-unknown-none
PROFILE := release
# O mesmo diretório que o cargo usa. Sem isto, `rust-objcopy` lê um ELF velho
# quando CARGO_TARGET_DIR aponta para fora de `target/`.
CARGO_TARGET_DIR ?= target
ELF     := $(CARGO_TARGET_DIR)/$(TARGET)/$(PROFILE)/rasppios
KERNEL  := boot/kernel8.img
IMG     := rasppios.img
DIST    := rasppios-dist.img

.PHONY: all build kernel run check firmware image dist preview write clean

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

# Imagem inicializável de distribuição: MBR + FAT32 + os cinco arquivos.
dist: kernel firmware
	bash scripts/dist-image.sh $(DIST)

# Prévia com janela, depois de gravar e desmontar o cartão. É o que o Pi
# deve mostrar. Aceita PREVIEW_SD=<imagem-do-cartao>.
preview: kernel
	bash scripts/preview.sh $(if $(PREVIEW_SD),$(PREVIEW_SD),)

# Copia para o exFAT já montado. Ex.: make write DEST=/media/vladimir/PENDRIVE
write: kernel firmware
	@test -n "$(DEST)" || { echo "uso: make write DEST=/media/vladimir/PENDRIVE"; exit 1; }
	bash scripts/install-boot.sh $(DEST)

clean:
	cargo clean
	rm -f $(KERNEL) $(IMG) $(DIST)
