#!/usr/bin/env bash
# Amostra exFAT com os mesmos arquivos do pendrive. Não é o meio de gravação:
# o pendrive real já está formatado e recebe os arquivos por install-boot.sh.
set -euo pipefail
cd "$(dirname "$0")/.."

IMG="${1:-rasppios.img}"
for f in boot/bootcode.bin boot/start.elf boot/fixup.dat boot/config.txt boot/kernel8.img; do
    [[ -f $f ]] || { echo "erro: falta $f"; exit 1; }
done

rm -f "$IMG"
truncate -s 64M "$IMG"
mkfs.exfat -n RASPPOS "$IMG" >/dev/null

mnt=$(mktemp -d)
trap 'sudo umount "$mnt" 2>/dev/null || true; rmdir "$mnt"' EXIT
sudo mount -o loop "$IMG" "$mnt"
for f in bootcode.bin start.elf fixup.dat config.txt kernel8.img; do
    sudo cp "boot/$f" "$mnt/"
done
sync
sudo umount "$mnt"
trap - EXIT
rmdir "$mnt"

echo "amostra pronta: $IMG (exFAT, rótulo RASPPOS, 64 MiB)"
echo "pendrive já formatado:"
echo "  make write DEST=/media/vladimir/PENDRIVE"
