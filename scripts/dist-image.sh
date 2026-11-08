#!/usr/bin/env bash
# Gera a imagem de distribuição do projeto: um cartão FAT32 com tabela de
# partições MBR, partição inicializável e os cinco arquivos de boot do Pi.
# O resultado é gravado com `dd` e inicia sem preparo adicional.
#
# Não precisa de root nem de loop device: a partição é montada off-line com
# mtools, usando um offset em setores.
set -euo pipefail
cd "$(dirname "$0")/.."

IMG="${1:-rasppios-dist.img}"
SIZE_MB="${SIZE_MB:-64}"
# 1 MiB de alinhamento, como recomendam as ferramentas do Raspberry Pi.
PART_START_SECTORS=2048
SECTOR=512

for tool in mkfs.vfat mmd mcopy sfdisk; do
    command -v "$tool" >/dev/null || { echo "erro: falta $tool"; exit 1; }
done
for f in boot/bootcode.bin boot/start.elf boot/fixup.dat boot/config.txt boot/kernel8.img; do
    [[ -f $f ]] || { echo "erro: falta $f"; exit 1; }
done

rm -f "$IMG"
truncate -s "${SIZE_MB}M" "$IMG"

# Tabela MBR com uma partição FAT32 LBA, marcada como inicializável.
sfdisk "$IMG" >/dev/null <<EOF
label: dos
unit: sectors

start=${PART_START_SECTORS}, size=+, type=c, bootable
EOF

offset=$((PART_START_SECTORS * SECTOR))
part_bytes=$((SIZE_MB * 1024 * 1024 - offset))

# O sistema de arquivos é criado numa janela deslocada dentro da imagem.
mkfs.vfat --offset "$PART_START_SECTORS" \
    -F 32 \
    -n RASPPIOS \
    "$IMG" >/dev/null

export MTOOLS_SKIP_CHECK=1
export MTOOLS_NO_VFAT=0
mcopy -i "$IMG@@$offset" boot/bootcode.bin ::bootcode.bin
mcopy -i "$IMG@@$offset" boot/start.elf ::start.elf
mcopy -i "$IMG@@$offset" boot/fixup.dat ::fixup.dat
mcopy -i "$IMG@@$offset" boot/config.txt ::config.txt
mcopy -i "$IMG@@$offset" boot/kernel8.img ::kernel8.img

echo "imagem de distribuição pronta: $IMG"
echo "  tamanho: ${SIZE_MB} MiB"
echo "  partição 1: FAT32, inicializável, início no setor ${PART_START_SECTORS}"
echo "  espaço na partição: ${part_bytes} bytes"

# Confere o que foi gerado em vez de confiar só no código de saída das
# ferramentas: a partição precisa estar marcada como inicializável e os
# cinco arquivos precisam existir com tamanho maior que zero.
part_line=$(sfdisk -d "$IMG" | grep -E "^$IMG[0-9]+ *:" || true)
[[ -n $part_line ]] || { echo "erro: nenhuma partição na imagem"; exit 1; }
[[ $part_line == *"type=c"* ]] || { echo "erro: partição não é FAT32 LBA: $part_line"; exit 1; }
[[ $part_line == *bootable* ]] || { echo "erro: partição não está marcada como inicializável"; exit 1; }

for f in bootcode.bin start.elf fixup.dat config.txt kernel8.img; do
    listing=$(MTOOLS_SKIP_CHECK=1 mdir -i "$IMG@@$offset" "::$f" 2>/dev/null || true)
    [[ -n $listing ]] || { echo "erro: $f ausente na imagem"; exit 1; }
done

echo "verificação: partição inicializável e 5 arquivos presentes"
echo "gravar com:"
echo "  sudo dd if=$IMG of=/dev/sdX bs=4M conv=fsync status=progress"
