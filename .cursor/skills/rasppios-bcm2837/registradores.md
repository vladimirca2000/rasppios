# Registradores usados nos Marcos 1, 3, 5 e 6

Fonte: BCM2835 ARM Peripherals, 06 February 2012. Endereço de barramento `0x7Enn_nnnn`. No Pi 3 o ARM usa `0x3Fnn_nnnn`.

| Registrador | Barramento | Físico no Pi 3 | Uso |
|---|---|---|---|
| GPFSEL1 | `0x7E20_0004` | `0x3F20_0004` | GPIO14 bits 12–14, GPIO15 bits 15–17. ALT5 = `010` |
| GPPUD | `0x7E20_0094` | `0x3F20_0094` | pull-up = `0b10`, depois 150 ciclos |
| GPPUDCLK0 | `0x7E20_0098` | `0x3F20_0098` | bits 14 e 15 |
| AUX_ENABLES | `0x7E21_5004` | `0x3F21_5004` | bit 0 habilita a miniUART |
| AUX_MU_IO | `0x7E21_5040` | `0x3F21_5040` | dado TX/RX, 8 bits válidos |
| AUX_MU_IER | `0x7E21_5044` | `0x3F21_5044` | `0` enquanto não há IRQ |
| AUX_MU_IIR | `0x7E21_5048` | `0x3F21_5048` | escrita com bit 1 limpa a FIFO de recepção |
| AUX_MU_LCR | `0x7E21_504C` | `0x3F21_504C` | `3` = 8 bits |
| AUX_MU_MCR | `0x7E21_5050` | `0x3F21_5050` | `0` |
| AUX_MU_LSR | `0x7E21_5054` | `0x3F21_5054` | bit 0 = dado pronto; bit 5 = transmissor aceita 1 byte |
| AUX_MU_CNTL | `0x7E21_5060` | `0x3F21_5060` | `3` liga TX e RX |
| AUX_MU_BAUD | `0x7E21_5068` | `0x3F21_5068` | `270` a 250 MHz para 115200 |
| ST_CS | `0x7E00_3000` | `0x3F00_3000` | escrita com 1 no bit limpa o match. Bit 1 = canal 1 |
| ST_CLO | `0x7E00_3004` | `0x3F00_3004` | contador livre, 32 bits de baixo |
| ST_C1 | `0x7E00_3010` | `0x3F00_3010` | compare do canal 1 |
| IRQ_PENDING1 | `0x7E00_B204` | `0x3F00_B204` | bit 1 pendente = match do canal 1 |
| IRQ_ENABLE1 | `0x7E00_B210` | `0x3F00_B210` | escrever 1 no bit 1 habilita a IRQ |

`AUX_MU_LSR` bit 0, “Data ready”, página 15: ligado se a FIFO de recepção tem ao menos 1 símbolo. Bit 5, “Transmitter empty”, mesma página. A leitura de `AUX_MU_IO` só acontece com o bit 0 ligado; no QEMU 8.2 essa leitura consome um byte mesmo com a FIFO vazia.

`AUX_MU_IIR` escrita, bit 1, página 13: limpa a FIFO de recepção. O mapa da página 8 coloca esse registrador em `0x7E21_5048`. O título da página 13 troca o nome com o `AUX_MU_IER`; o endereço e o texto do resumo é que valem. O QEMU 8.2 zera a FIFO ao escrever `0x2` nesse offset.

A página 10 pede a função GPIO antes de habilitar a miniUART. Com o pino de RX ainda em entrada e nível baixo, o receptor trata isso como start bit e enche a FIFO de `0x00`.

O system timer está na página 172: base `0x7E00_3000`, `CS` limpa o match ao escrever 1, `CLO` é o contador livre e `C1` é o compare do canal 1. A página 112 coloca o controlador em `0x7E00_B000`, com `IRQ pending 1` no offset `0x204` e `Enable IRQs 1` no offset `0x210`. A tabela da página 113 deixa os nomes das IRQ 0–15 em branco. O QEMU 8.2 liga o compare `n` na IRQ `n` desse pending; o canal 1 é o bit 1. Os canais 0 e 2 ficam de fora porque o VideoCore os usa no Pi. O `CLO` do QEMU 8.2 anda a 1 MHz. O tick do marco 5 é um match a cada 10 000 contas, 100 Hz.

O mailbox do canal de propriedades não está no PDF do BCM2835. O QEMU 8.2.2 (`hw/misc/bcm2835_mbox.c`) mapeia o bloco em `0x3F00_B800` e usa leitura `0x3F00_B880`, status `0x3F00_B898` e escrita `0x3F00_B8A0`. Bit 31 = cheio, bit 30 = vazio. O canal 8 recebe a tag `0x00010005` (`GET_ARM_MEMORY`).

Baud: `divisor = clock / (8 * baud) - 1`.

Não inventar offset. Registrador novo entra nesta tabela com a página do PDF antes de ir para o código.
