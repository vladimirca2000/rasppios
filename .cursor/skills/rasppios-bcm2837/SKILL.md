---
name: rasppios-bcm2837
description: >-
  Contrato de hardware do rasppios no Raspberry Pi 3B+ (BCM2837B0) e no
  QEMU raspi3b. Use ao escrever boot, linker, miniUART, GPIO, config.txt,
  kernel8.img, exceção, timer ou mapa de memória; ao escolher EL, base
  de periféricos ou o serial do QEMU.
---

# BCM2837 no rasppios

Fatos deste repositório. O detalhe de registrador está em [registradores.md](registradores.md).

## Boot

- O firmware oficial carrega `kernel8.img` em `0x80000` e salta para o primeiro byte. O ELF tem o mesmo `e_entry`.
- `boot/config.txt`: `arm_64bit=1`, `kernel=kernel8.img`, `enable_uart=1`, `core_freq=250`.
- `kernel.ld` coloca `.text.boot` em `0x80000`, zera o contrato do BSS e deixa 128 KiB de pilha para o núcleo 0.
- `src/boot.s` dá 32 KiB de pilha a cada núcleo e estaciona quem tem `MPIDR_EL1` Aff0 diferente de 0. Só o núcleo 0 entra em `rust_main`.
- Não há GIC genérico. Interrupções futuras usam o controlador do BCM2837.

## Exception level

- `make check` sobe a imagem raw. O stub do `raspi3b` entra em **EL2**.
- `make run` carrega o ELF direto e entra em **EL3**.
- O Pi 3B+ real deixa o núcleo em **EL2**. Ler `CurrentEL`. Não assumir o nível.

## UART

- O conector serial é a miniUART (UART1) em GPIO14/GPIO15, função ALT5 (`0b010`).
- PL011 (UART0) fica no Bluetooth com `enable_uart=1` e sem `dtoverlay=disable-bt`.
- No QEMU `raspi3b` a miniUART é o segundo serial: `-serial null -serial stdio`.
- 115200 8N1 com `core_freq=250` usa o divisor `270`.
- Base física no Pi 2/3: `0x3F00_0000`. O PDF do BCM2835 publica o barramento `0x7Enn_nnnn` e o físico antigo `0x20nn_nnnn`. Traduzir `0x7E` para `0x3F` neste SoC.

## Mídia

- O pendrive de teste permanece exFAT. Copiar arquivos; não formatar.
- A documentação de boot descreve a partição de carregamento como FAT16/FAT32. exFAT pode não ser lida pelo boot ROM. Registrar o resultado no hardware em `agent.md`.

## QEMU

```bash
make check
qemu-system-aarch64 -M raspi3b -kernel boot/kernel8.img \
  -serial null -serial stdio -display none -no-reboot
```

Máquina `raspi3b`, não existe `raspi3b+`. Conferir a saída com `scripts/check-qemu.sh`.
