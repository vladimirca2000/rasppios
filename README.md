# rasppios

Kernel bare-metal para o **Raspberry Pi 3B+** (BCM2837, 4× Cortex-A53), em Rust
`no_std` e assembly AArch64. Não há Linux. O firmware oficial da placa só
carrega `kernel8.img`; o sistema daqui em diante é nosso.

## O que já foi feito

### Marco 1 — projeto inicializável

- Crate `no_std` para `aarch64-unknown-none`, linkada em `0x80000`.
- `src/boot.s` escolhe o núcleo 0, arma a pilha, zera o BSS e chama `rust_main`.
- A miniUART (GPIO14/15, 115200 8N1) imprime o banner e o exception level.
- `make check` sobe `boot/kernel8.img` no QEMU `raspi3b` e confere o banner.
- `boot/config.txt` pede AArch64, `kernel8.img`, `enable_uart=1` e `core_freq=250`.
- O pendrive de teste (`/dev/sda1`, exFAT, montado em `/media/vladimir/PENDRIVE`)
  recebe `bootcode.bin`, `start.elf`, `fixup.dat`, `config.txt` e `kernel8.img`
  sem ser reformatado. `rasppios.img` é uma amostra exFAT de 64 MiB com os
  mesmos arquivos. O kernel ainda não lê exFAT.

Ainda falta, para fechar o Marco 1 no hardware: ligar esse pendrive no Pi 3B+ e ler o banner na UART. A documentação de boot descreve a partição de carregamento como FAT/FAT32; o volume permanece exFAT porque é o formato em que o pendrive foi gravado.

### Marco 2 — um núcleo no kernel

- `src/boot.s` lê `MPIDR_EL1`, dá 32 KiB de pilha ao núcleo e marca a chegada.
- Núcleo diferente de 0 entra em `WFE` e não chama `rust_main`.
- O núcleo 0 zera o BSS e imprime quem chegou.
- `make check` sobe a imagem duas vezes e exige núcleo 0 nas duas.

No QEMU 8.2 e no firmware do Pi, os núcleos 1–3 não saltam para `0x80000`. A saída esperada traz `chegada: 0=1 1=0 2=0 3=0` e `kernel somente no nucleo 0`. A janela do QEMU continua preta.

### Marco 3 — console serial

- A miniUART lê um byte quando `AUX_MU_LSR` bit 0 está ligado e ecoa a linha.
- GPIO14/15 vão para ALT5 antes de habilitar o receptor, e a FIFO de recepção é esvaziada na inicialização.
- O prompt `rasppios>` responde a `help`, `el`, `nucleo` e `eco`.
- `make check` envia esses comandos e exige `el: EL2`, a chegada do núcleo 0 e o eco.

Ainda falta, para fechar o Marco 3 no hardware: abrir o terminal na UART e ver o prompt responder. O exemplo abaixo manda `help`.

```bash
picocom -b 115200 /dev/ttyUSB0
```

### Marco 4 — exceções e falhas

- A tabela de vetores fica alinhada em 2 KiB e é instalada em `VBAR_ELx` do nível em que o núcleo entrou.
- `falha` provoca `BRK #1`. O relato traz a causa, o ESR e os registradores, e o console continua.
- `panico` escreve `PANIC`, o ponto do código e a mensagem, e fica parado. Não há reinício.

Ainda falta, para fechar o Marco 4 no hardware: mandar `falha` e `panico` pela UART física e ler o mesmo relato.

### Marco 5 — temporizador e interrupções

- O canal 1 do system timer compara com `CLO` a cada 10 000 contas. No QEMU isso é 100 Hz.
- A IRQ entra pelo controlador do BCM2837, no bit 1, e o vetor `0x280` só conta o tick e volta.
- `ticks` mostra o contador e o `CLO`.

Ainda falta, para fechar o Marco 5 no hardware: mandar `ticks`, esperar e mandar de novo, e ver o contador subir sem depender do QEMU.

### Marco 6 — alocação de memória física

- O mailbox, canal 8, lê o tamanho de RAM que o firmware entrega ao ARM. No QEMU isso é `0x3C000000` bytes a partir do endereço 0.
- O heap fica entre o fim das pilhas e esse limite. O MMIO em `0x3F000000` fica de fora.
- `mem` aloca dois blocos de 4096 bytes, mostra que não se sobrepõem, libera o primeiro e o recebe de novo no mesmo endereço.

Ainda falta, para fechar o Marco 6 no hardware: mandar `mem` pela UART física e conferir o tamanho de RAM que o Pi responder.

### Marco 7 — MMU e memória virtual

- A tradução é identidade, com grânulo de 4 KiB. Código e dados não compartilham a mesma permissão de página.
- A RAM vai até o limite que o mailbox informou. O intervalo dos periféricos é device. Entre os dois há um buraco sem tradução.
- `pagina` escreve e lê uma página mapeada. O acesso a `0x3E000000` gera abort de dados, o relato mostra o `FAR`, e o console continua.

Ainda falta, para fechar o Marco 7 no hardware: mandar `pagina` pela UART física e ler o mesmo relato, com a miniUART ainda viva.

### Marco 8 — primitivas de concorrência

- Uma trava com `compare_exchange` protege um contador comum. A carga e o armazenamento são separados.
- O núcleo 0 acorda os outros pelo spin table em `0xd8` e um `sev`.
- `trava` espera os quatro núcleos. A soma tem de ser 4000.

Ainda falta, para fechar o Marco 8 no hardware: mandar `trava` pela UART física e ler `nucleos 4 soma 4000`.

### Marco 9 — primeiro escalonador

- Duas tarefas de kernel têm pilha própria e cedem a vez com `yield`.
- A troca guarda os registradores que a tarefa precisa recuperar e o ponteiro de pilha.
- `tarefas` mostra a ordem `abababab` e confirma que o estado de cada uma sobreviveu.

Ainda falta, para fechar o Marco 9 no hardware: mandar `tarefas` pela UART física e ler a mesma ordem.

### Marco 10 — preempção e prioridade

- A IRQ do timer troca a tarefa quando a preempção está ligada.
- A tarefa de maior prioridade corre primeiro. Na mesma prioridade, elas alternam.
- `preempcao` mostra a de prioridade alta antes, e as duas fatias em `abababab` sem `yield` na espera.

Ainda falta, para fechar o Marco 10 no hardware: mandar `preempcao` pela UART física e ler as mesmas duas linhas, com o timer ainda avançando.

## Compilar e testar no QEMU

```bash
make check
```

Saída esperada de `make check` (sobe `boot/kernel8.img`, o mesmo arquivo do cartão):

```
rasppios: marco 10
kernel AArch64 carregado em 0x80000
exception level: EL2
nucleo em execucao: 0
chegada: 0=1 1=0 2=0 3=0
timer: 100 Hz
mmu: ligada
kernel somente no nucleo 0
rasppios> preempcao
preempcao: alta antes
fatias: abababab
```

Esse caminho passa pelo stub de boot do `raspi3b` e chega em **EL2**, o mesmo
nível que o firmware deixa no Pi 3B+ real. `make run` carrega o ELF direto, sem
esse stub, e o banner mostra **EL3**. O kernel lê `CurrentEL`; não assuma o nível.

O modelo `raspi3b` liga o PL011 no serial 0 e a miniUART no serial 1, por isso
`-serial null -serial stdio`.

## Pendrive

O volume de teste já está em exFAT. A gravação copia os arquivos e deixa o
formato como está:

```bash
make write DEST=/media/vladimir/PENDRIVE
```

`make image` gera `rasppios.img`, uma amostra exFAT de 64 MiB com os mesmos
cinco arquivos. O firmware, quando encontra o volume, carrega o kernel em
`0x80000`.

## Documentação do Pi 3B+

- [Placas clássicas](https://www.raspberrypi.com/documentation/computers/sbcs.html): Pi 3B+ com BCM2837B0, 4× Cortex-A53 a 1,4 GHz e 1 GB de LPDDR2.
- [Processadores](https://www.raspberrypi.com/documentation/computers/processors.html): o BCM2837B0 repete a arquitetura do BCM2837, com clock mais alto.
- [Hardware e boot](https://www.raspberrypi.com/documentation/computers/raspberry-pi.html): no BCM2837 o boot ROM carrega `bootcode.bin`; o modo só com esse arquivo pede cartão FAT32.
- [config.txt](https://www.raspberrypi.com/documentation/computers/config_txt.html): `arm_64bit`, `kernel` e `enable_uart` (GPIO 14 e 15, pinos 8 e 10).
- [BCM2835 ARM Peripherals](https://www.raspberrypi.org/app/uploads/2012/02/BCM2835-ARM-Peripherals.pdf): mapa dos periféricos. Endereços de barramento `0x7Enn_nnnn`; no documento original o físico é `0x20nn_nnnn`. No Pi 3 o acesso pelo ARM usa `0x3Fnn_nnnn`, que é o mapa com o qual a miniUART respondeu no QEMU.

`boot/config.txt` pede AArch64 (`arm_64bit=1`), nomeia `kernel8.img`, liga a
miniUART (`enable_uart=1`) e fixa o clock do núcleo em 250 MHz para o divisor
270 produzir 115200 baud.

### Console serial

| Adaptador USB-TTL | Raspberry Pi 3B+ |
|---|---|
| RX | GPIO14 (TX do Pi) |
| TX | GPIO15 (RX do Pi) |
| GND | GND (pino 6) |

Não ligue 3,3 V nem 5 V do adaptador. Terminal: `picocom -b 115200 /dev/ttyUSB0`
(8N1). O mesmo banner do QEMU deve aparecer; o exception level no hardware é EL2.

## Skills

Em `.cursor/skills/`:

- `rasppios-etapa` — atualizar `agent.md` e este arquivo, rodar `make check` e copiar para o exFAT.
- `rasppios-bcm2837` — endereço `0x80000`, miniUART, EL2/EL3 e a base `0x3F00_0000`.

## Layout

```
├── kernel.ld            # .text em 0x80000, pilha de 128 KiB
├── src/boot.s           # _start: núcleo 0, pilha, BSS
├── src/main.rs          # banner e entrada do console
├── src/console.rs       # comandos de diagnóstico, falha e pânico
├── src/exception.rs     # relato de ESR, ELR, SPSR, FAR e x0
├── src/timer.rs         # system timer canal 1, 100 Hz
├── src/physmem.rs       # mapa da RAM e alocador físico
├── src/mmu.rs           # tabelas de página e MMU de estágio 1
├── src/sync.rs          # trava e teste entre os quatro núcleos
├── src/sched.rs         # tarefas, prioridade e preempção pelo timer
├── src/vectors.s        # tabela VBAR, 2 KiB, 16 entradas
├── src/uart.rs          # miniUART: transmissão, recepção e formatação
├── boot/config.txt      # contrato com o firmware do Pi
└── scripts/             # QEMU, checagem, firmware e imagem
```
