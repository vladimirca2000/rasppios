#!/usr/bin/env bash
# Blobs do firmware oficial. Não versionar: são binários de terceiros.
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p boot

BASE="https://raw.githubusercontent.com/raspberrypi/firmware/master/boot"

for f in bootcode.bin start.elf fixup.dat; do
    if [[ ! -f "boot/$f" ]]; then
        echo "baixando $f..."
        curl -fsSL "$BASE/$f" -o "boot/$f"
    fi
done

ls -lh boot/bootcode.bin boot/start.elf boot/fixup.dat
