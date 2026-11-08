#!/usr/bin/env bash
# Abre o QEMU com janela para conferir, antes de ir ao Raspberry Pi, o que a
# tela e o console devem mostrar. Roda depois da gravação do cartão e da
# desmontagem, para que o preview use exatamente o kernel8.img gravado.
#
# Uso: bash scripts/preview.sh [imagem-de-cartao]
#
# Sem argumento, cria uma imagem de cartão nova (zerada) em /tmp, que é o que
# o kernel encontra num cartão sem RPFS: ele mostra "fsck: sem sistema" e
# `arquivos` formata a região. Passe a imagem do cartão para reproduzir um
# estado já formatado.
set -euo pipefail
cd "$(dirname "$0")/.."

KERNEL="boot/kernel8.img"
[[ -f "$KERNEL" ]] || { echo "erro: falta $KERNEL (rode make kernel)"; exit 1; }

# O número do marco vem do código, não de um literal aqui, para a mensagem
# nunca divergir do que a tela mostra.
MARCO=$(sed -n 's/^pub const MARCO: u8 = \([0-9]\+\).*/\1/p' src/version.rs)
[[ -n $MARCO ]] || { echo "erro: não achei MARCO em src/version.rs"; exit 1; }

if [[ $# -ge 1 ]]; then
    SD="$1"
    [[ -f "$SD" ]] || { echo "erro: imagem de cartão não encontrada: $SD"; exit 1; }
    OWN_IMAGE=0
else
    SD="$(mktemp -t rasppios-preview-XXXXXX.img)"
    # 4 MiB cobrem o RPFS (blocos 60..82) e deixam o resto zerado.
    dd if=/dev/zero of="$SD" bs=1M count=4 status=none
    OWN_IMAGE=1
fi

if [[ -z ${DISPLAY:-} && -z ${WAYLAND_DISPLAY:-} ]]; then
    echo "erro: sem DISPLAY/WAYLAND_DISPLAY; esta prévia precisa de ambiente gráfico"
    echo "      para conferir só o texto use: make run"
    [[ $OWN_IMAGE -eq 1 ]] && rm -f "$SD"
    exit 1
fi

echo "prévia do rasppios"
echo "  kernel: $KERNEL"
echo "  cartão: $SD"
echo
echo "o que conferir nesta janela:"
echo "  aba bcm2835-fb — barra 'rasppios marco $MARCO', painel verde 'status',"
echo "                   painel azul 'terminal' e painel vermelho 'eventos'"
echo "  aba serial1    — banner, logs I:/W:/E: e o prompt 'rasppios>'"
echo "  digite 'help' na aba serial1 para confirmar que o console responde"
echo "  digite 'modos' para ver o modo de vídeo atual e a tabela de modos"
echo "  o teclado USB do QEMU (monitor/sendkey) alimenta a GUI"
echo
echo "feche a janela para encerrar. O mesmo conteúdo deve aparecer no Pi."
echo

cleanup() {
    if [[ $OWN_IMAGE -eq 1 ]]; then
        rm -f "$SD"
    fi
}
trap cleanup EXIT

exec qemu-system-aarch64 \
    -M raspi3b \
    -kernel "$KERNEL" \
    -drive "if=sd,format=raw,file=$SD" \
    -device usb-kbd \
    -serial null \
    -serial vc:120Cx40C \
    -monitor none \
    -display gtk,show-tabs=on,zoom-to-fit=on \
    -no-reboot
