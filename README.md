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
- O cartão SD de teste no adaptador USB (`/dev/sda1`, FAT32, montado em
  `/media/vladimir/PENDRIVE`) recebe `bootcode.bin`, `start.elf`, `fixup.dat`,
  `config.txt` e `kernel8.img`
  sem ser reformatado. `rasppios.img` é uma amostra exFAT de 64 MiB com os
  mesmos arquivos. O kernel ainda não lê exFAT.

Em 2026-11-06 o Pi 3B+ iniciou esse cartão pelo adaptador USB e a UART física mostrou o banner. Depois, a partição foi reformatada em FAT32 para permitir o boot pelo slot SD. A documentação de boot descreve a partição de carregamento como FAT/FAT32.

### Marco 2 — um núcleo no kernel

- `src/boot.s` lê `MPIDR_EL1`, dá 32 KiB de pilha ao núcleo e marca a chegada.
- Núcleo diferente de 0 entra em `WFE` e não chama `rust_main`.
- O núcleo 0 zera o BSS e imprime quem chegou.
- `make check` sobe a imagem duas vezes e exige núcleo 0 nas duas.

No QEMU 8.2 e no firmware do Pi, os núcleos 1–3 não saltam para `0x80000`. A saída esperada traz `chegada: 0=1 1=0 2=0 3=0` e `kernel somente no nucleo 0`. Em 2026-11-06 a UART física do Pi mostrou essas duas linhas. A janela do QEMU continua preta.

### Marco 3 — console serial

- A miniUART lê um byte quando `AUX_MU_LSR` bit 0 está ligado e ecoa a linha.
- GPIO14/15 vão para ALT5 antes de habilitar o receptor, e a FIFO de recepção é esvaziada na inicialização.
- O prompt `rasppios>` responde a `help`, `el`, `nucleo` e `eco`.
- `make check` envia esses comandos e exige `el: EL1`, a chegada do núcleo 0 e o eco.

Em 2026-11-06 a UART física aceitou um comando e voltou ao prompt. O exemplo abaixo manda `help`.

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

### Marco 11 — modo de usuário

- O núcleo desce de EL2 (firmware e `make check`) ou EL3 (`make run`) para EL1 antes do banner.
- O kernel continua na tradução identidade, sem permissão para o EL0.
- O programa de usuário ocupa `0x3C200000`: uma página de código, uma de dados e uma de pilha.
- `usuario` lê a própria página e, ao carregar `0x80000`, toma abort de dados. O console segue.

Em 2026-11-06 a UART física do Pi 3B+ mostrou `proprio: ok`, `esr: 0x9200000f`, `far: 0x0000000000080000`, `usuario: kernel negado` e o prompt de novo.

### Marco 12 — chamadas de sistema

- O EL0 usa `SVC` com o número em `x8`: escrever, ler, consultar o `CLO` e encerrar.
- O kernel só segue um ponteiro que caiba na janela do usuário. `0x80000` é recusado.
- `chamada` escreve `saida: ok`, lê um byte da UART, mostra o tempo e termina com status 0.

Em 2026-11-06 a UART física do Pi 3B+ mostrou `saida: ok`, `ponteiro: negado`, `tempo: 131104175` e `fim: 0`, com o prompt de volta. O byte da leitura foi um retorno de carro, então a linha `entrada` não mostra um caractere.

### Marco 13 — teclado USB

- O DWC2 (`0x3F980000`) enumera um hub de um nível e um teclado HID de boot.
- As teclas entram no mesmo console da miniUART.
- Sem teclado, o boot segue e imprime `teclado: ausente`.

O `make check` liga `-device usb-kbd`. O monitor manda `eco usb` e a resposta é `eco: usb`.

Em 2026-11-06 um teclado USB no Pi 3B+ fez o boot mostrar `teclado: hid`. A linha `eco usb` digitada nele voltou como `eco: usb`.

### Marco 14 — blocos no cartão SD

- O SDHOST (`0x3F202000`) lê e grava blocos de 512 bytes.
- Os GPIO 48–53 ficam em ALT0, que é o caminho do microSD no Pi 3B+.
- `bloco` mostra o começo do bloco 0, grava `0xA5` no bloco 1 e devolve o conteúdo anterior.

O `make check` usa uma imagem crua de 4 MiB só de zeros. A saída do comando é `leitura: 00000000`, `escrita: ok` e `restauro: ok`.

Em 2026-11-06 o cartão no slot do Pi 3B+ respondeu `leitura: 00000000`, `escrita: ok` e `restauro: ok`. O bloco 1 voltou ao conteúdo anterior.

### Marco 15 — sistema de arquivos mínimo

- A marca `RPFS` fica no bloco 64. A raiz, o mapa de blocos e os arquivos vêm em seguida.
- Há diretório e arquivo. Cada arquivo cabe num bloco de 512 bytes.
- `arquivos` cria `ola` com o texto `rasppios` e `notas/item` com o texto `dentro`.
- No boot seguinte a mesma imagem responde `persistiu: ok`.

Em 2026-11-06 a primeira execução no Pi 3B+ mostrou `montagem: nova`, `lista: ola notas`, `leitura: rasppios` e `sub: dentro`. O cartão foi posteriormente reformatado em FAT32, apagando o RPFS. É necessário mandar `arquivos` novamente e repetir a sequência do shell no Pi para fechar este marco.

### Marco 16 — carregador de programas

- O arquivo `prog` guarda uma imagem `RPEX`: marca, versão, entrada e o código.
- `roda` lê esse arquivo do cartão, copia o código para `0x3C200000` e entra em EL0.
- A aplicação escreve `app: ok` e termina com status 7.
- Na primeira vez o arquivo é gravado. Depois de reiniciar, `roda: disco` mostra que ele veio do cartão.

Em 2026-11-06 o Pi 3B+ mostrou `roda: gravou` e, em seguida, `roda: disco`. As duas terminaram com `app: ok` e `saida: 7`. A segunda leu o `prog` que já estava no cartão.

### Marco 17 — shell

- `ls` lista a raiz ou um diretório. `cat` lê um arquivo, também em `notas/item`.
- `echo` escreve o resto da linha. `eco` faz o mesmo.
- `run prog` carrega a imagem que está no cartão e mostra o status.
- O teclado USB escreve na mesma linha que o serial.

O `make check` compara `ls: ola notas prog`, `ls: item`, `cat: rasppios`, `cat: dentro`, `echo: shell` e `run: ok 7`.

Ainda falta, para fechar o Marco 17 no hardware: repetir essa sequência na UART do Pi, pelo serial ou pelo teclado, depois da formatação FAT32.

### Marco 18 — saída gráfica

- O framebuffer negociado no QEMU `raspi3b` é `640x480`, RGB565, com `stride 1280`.
- O comando `tela` desenha fundo azul-escuro, faixa azul, três retângulos e os textos `rasppios marco 18`, `framebuffer: 640x480 rgb565` e `UART continua ativa`.
- No QEMU, a resposta `tela: ok 640x480 rgb565` fica na aba `serial1`; a cena fica na aba `bcm2835-fb`.

Reprodução visual no QEMU:

```bash
qemu-system-aarch64 -M raspi3b -kernel boot/kernel8.img \
  -drive if=sd,format=raw,file=/tmp/rasppios-sd.img \
  -device usb-kbd -serial null -serial vc:100Cx30C \
  -monitor none -display gtk,show-tabs=on,zoom-to-fit=on -no-reboot
```

Ainda não há evidência física do monitor no Raspberry Pi 3B+; o aceite de vídeo no hardware permanece aberto.

## Compilar e testar no QEMU

```bash
make check
```

Saída esperada de `make check` (sobe `boot/kernel8.img`, o mesmo arquivo do cartão):

```
rasppios: marco 15
kernel AArch64 carregado em 0x80000
exception level: EL1
nucleo em execucao: 0
chegada: 0=1 1=0 2=0 3=0
timer: 100 Hz
mmu: ligada
kernel somente no nucleo 0
teclado: hid
rasppios> arquivos
montagem: nova
lista: ola notas
leitura: rasppios
sub: dentro
rasppios> roda
roda: gravou
app: ok
saida: 7
rasppios> ls
ls: ola notas prog
rasppios> cat ola
cat: rasppios
rasppios> echo shell
echo: shell
rasppios> run prog
run: prog
app: ok
run: ok 7
```

No boot seguinte, com o mesmo cartão:

```
rasppios> arquivos
montagem: pronta
lista: ola notas
leitura: rasppios
sub: dentro
persistiu: ok
rasppios> roda
roda: disco
app: ok
saida: 7
```

O stub do `raspi3b` entra em **EL2**, o mesmo nível que o firmware deixa no
Pi 3B+ real. `make run` carrega o ELF direto e entra em **EL3**. Os dois
caminhos descem a **EL1** antes do banner. O `el` do console é o nível em
que o kernel ficou, não o nível em que o firmware entregou o núcleo.

O modelo `raspi3b` liga o PL011 no serial 0 e a miniUART no serial 1, por isso
`-serial null -serial stdio`.

## Pendrive

O volume de teste está em FAT32. A gravação copia os arquivos e deixa o
formato como está:

```bash
make write DEST=/media/vladimir/PENDRIVE
```

`make image` gera `rasppios.img`, uma amostra FAT32 de 64 MiB com os mesmos
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
(8N1). O mesmo banner do QEMU deve aparecer. O firmware deixa o núcleo em EL2; o kernel desce a EL1 e o banner mostra esse nível.

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
├── src/syscall.rs       # escrever, ler, tempo e encerrar a partir do EL0
├── src/user.rs          # programa EL0 e retorno ao kernel
├── src/user.s           # ERET para EL0 e o código da janela
├── src/vectors.s        # tabela VBAR, 2 KiB, 16 entradas
├── src/uart.rs          # miniUART: transmissão, recepção e formatação
├── src/usb.rs           # DWC2, hub de um nível e teclado HID de boot
├── src/sd.rs            # SDHOST: leitura e escrita de blocos de 512 bytes
├── src/fs.rs            # RPFS: diretório, arquivo, leitura e gravação
├── src/load.rs          # imagem RPEX e entrada em EL0
├── boot/config.txt      # contrato com o firmware do Pi
└── scripts/             # QEMU, checagem, firmware e imagem
```
