#!/usr/bin/env bash
# Sobe o ELF no QEMU raspi3b.
# PL011 = serial 0, miniUART = serial 1. O kernel usa o miniUART.
set -euo pipefail

KERNEL="${1:?uso: qemu.sh <caminho-do-elf-ou-kernel8.img>}"

exec qemu-system-aarch64 \
    -M raspi3b \
    -kernel "$KERNEL" \
    -serial null \
    -serial stdio \
    -display none \
    -no-reboot
