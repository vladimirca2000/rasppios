#!/usr/bin/env bash
# Copia o firmware e o kernel para um volume exFAT já montado.
# Não formata e não mexe na tabela de partições: o sistema fica no
# formato que o pendrive já tem.
set -euo pipefail
cd "$(dirname "$0")/.."

DEST="${1:-}"
[[ -d $DEST ]] || { echo "uso: bash scripts/install-boot.sh /ponto/de/montagem"; exit 1; }

src=$(findmnt -no SOURCE -T "$DEST")
fstype=$(findmnt -no FSTYPE -T "$DEST")
[[ $fstype == exfat ]] || { echo "erro: $DEST está em '$fstype'; este passo espera exFAT"; exit 1; }
case $src in
    /dev/nvme*|/dev/mmcblk0*)
        echo "erro: $src parece disco do sistema"
        exit 1
        ;;
esac

for f in boot/bootcode.bin boot/start.elf boot/fixup.dat boot/config.txt boot/kernel8.img; do
    [[ -f $f ]] || { echo "erro: falta $f"; exit 1; }
done

for f in bootcode.bin start.elf fixup.dat config.txt kernel8.img; do
    cp -v "boot/$f" "$DEST/"
done
sync
echo "arquivos copiados para $DEST ($src, exFAT)"
ls -lh "$DEST"
