---
name: rasppios-etapa
description: >-
  Conduz cada marco do rasppios, kernel bare-metal do Raspberry Pi 3B+.
  Use ao alterar código, imagem, config.txt ou decisão técnica; ao fechar
  uma etapa; ao atualizar agent.md ou README.md; ao rodar make check,
  make write ou o QEMU raspi3b; ao gravar o pendrive exFAT.
---

# Etapa do rasppios

O plano e o estado vivem em `agent.md`. O que já funciona vive no `README.md`.

## Ao mudar código, imagem ou decisão

1. Atualizar `agent.md`: marco, o que mudou, o que falta para o aceite.
2. Atualizar `README.md`: o que já está feito e o comando para reproduzir.
3. Rodar `make check` a partir da raiz do repositório. O aceite no QEMU é o banner `rasppios: marco 1` vindo de `boot/kernel8.img`.
4. Se o kernel ou o `config.txt` mudou, copiar para o pendrive exFAT já montado:

```bash
make write DEST=/media/vladimir/PENDRIVE
```

Confirmar com `findmnt` que o destino é exFAT e que a origem não é `nvme`. `scripts/install-boot.sh` recusa outro sistema de arquivos e recusa os NVMe. Não formatar o pendrive e não usar `scripts/write-drive.sh`.

## Aceite

Um marco só fecha com as três checagens do `agent.md`: teste automatizado, boot no QEMU `raspi3b` e boot no Pi 3B+. O QEMU não substitui o Pi.

## Skills já instaladas

Antes de escrever a parte correspondente, ler a skill pessoal:

| Trabalho | Skill |
|---|---|
| `no_std`, panic, `alloc` | `rust-no-std` |
| alvo `aarch64-unknown-none` | `rust-cross` |
| `unsafe`, MMIO | `rust-unsafe-reasoning`, `mmio-and-bit-manipulation`, `embedded-volatile-and-memory-ordering` |
| registrador novo | `embedded-hw-register-datasheet-verification` |
| `_start`, link em `0x80000` | `bootloader-stages`, `embedded-linker-script`, `assembly-arm` |
| linha de comando do QEMU | `qemu-system-setup` |
| mapa do BCM2837 deste repositório | `rasppios-bcm2837` |
