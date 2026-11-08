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
  sem ser reformatado. `rasppios.img` é uma imagem de teste exFAT de 64 MiB
  com os mesmos arquivos, gerada pelo script atual; o kernel ainda não lê
  exFAT.

Antes dos testes, a partição foi convertida uma vez de exFAT para FAT32 para
permitir o boot pelo slot SD. Desde então, o cartão permanece em FAT32. A
documentação de boot descreve a partição de carregamento como FAT/FAT32.

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
- Até o Marco 18, o teclado USB escrevia na mesma linha que o serial; no
  Marco 19, a entrada USB passa a alimentar o terminal gráfico.

O `make check` compara `ls: ola notas prog`, `ls: item`, `cat: rasppios`, `cat: dentro`, `echo: shell` e `run: ok 7`.

Ainda falta, para fechar o Marco 17 no hardware: repetir essa sequência na UART do Pi, pelo serial ou pelo teclado, depois da formatação FAT32.

### Marco 18 — saída gráfica

- O framebuffer negociado no QEMU `raspi3b` é `640x480`, RGB565, com `stride 1280`.
- Durante a carga, o kernel desenha automaticamente fundo azul-escuro, faixa azul, três retângulos e os textos `rasppios marco 18`, `framebuffer: 640x480 rgb565` e `UART continua ativa`; o comando `tela` redesenha a mesma cena.
- No QEMU, a resposta `tela: ok 640x480 rgb565` fica na aba `serial1`; a cena fica na aba `bcm2835-fb`.

Reprodução visual no QEMU:

```bash
qemu-system-aarch64 -M raspi3b -kernel boot/kernel8.img \
  -drive if=sd,format=raw,file=/tmp/rasppios-sd.img \
  -device usb-kbd -serial null -serial vc:100Cx30C \
  -monitor none -display gtk,show-tabs=on,zoom-to-fit=on -no-reboot
```

Em 2026-11-07, o monitor físico do Raspberry Pi 3B+ exibiu a mesma cena automaticamente durante a carga. O Marco 18 está aceito no QEMU e no Pi.

### Marco 19 — interface gráfica inicial

- A GUI desenha uma barra superior e três janelas fixas: `status`,
  `terminal` e `eventos`.
- O foco começa no terminal e alterna com `Tab`; a borda da janela focada é
  destacada.
- O teclado USB envia caracteres, `Backspace` e `Enter` ao terminal gráfico,
  enquanto a UART permanece um console independente.
- O terminal tem histórico limitado e responde a `help`, `clear` e
  `echo texto`; comandos desconhecidos recebem uma resposta local.
- O teste automatizado envia `Tab` e `echo oi` pelo monitor do QEMU e mantém
  as regressões dos marcos anteriores.

Reprodução com janela gráfica:

```bash
qemu-system-aarch64 -M raspi3b -kernel boot/kernel8.img \
  -drive if=sd,format=raw,file=/tmp/rasppios-sd.img \
  -device usb-kbd -serial null -serial vc:100Cx30C \
  -monitor none -display gtk,show-tabs=on,zoom-to-fit=on -no-reboot
```

O teste físico no Raspberry Pi 3B+ ainda precisa confirmar a tela, o foco e o
terminal com teclado USB.

### Marco 20 — robustez e recuperação

O plano está em `docs/superpowers/plans/2026-11-07-marco-20-robustez.md`.
A implementação atual possui logs fixos, limites do RPFS, os comandos `logs`,
`limites`, `fsck`, `stress` e `verifica`, injeção controlada de falhas,
checksums no RPFS v1, metadados `RPV2` redundantes e journal básico com
recuperação no boot. O banner UART e a GUI identificam explicitamente o
`marco 20`.

No boot, o kernel executa `fsck` automaticamente. A GUI mostra
`robustez: logs`, `fsck journal`, `carga: 100 ciclos`, `v2: superblocos` e
`journal: ativo`; o painel de eventos exibe os logs mais recentes. O comando
`verifica` executa `fsck` e `stress` em sequência. O comando `falha apos N`
permite exercitar falhas determinísticas depois de N operações de SD.

O USB registra no painel e na UART se o host foi ativado e se um teclado HID
foi enumerado (`usb-host-ready`, `usb-keyboard-hid` ou o motivo da falha).
O driver atual ainda não controla os LEDs do teclado nem implementa entrada
de mouse; o mouse pode receber alimentação, mas não participa da GUI.

O QEMU e a regressão automatizada validam boot, persistência, `fsck`, 100
ciclos de carga, falhas de leitura/escrita e retorno ao prompt. Ainda falta
o teste físico de interrupção do journal nos três pontos transacionais e o
aceite final no Raspberry Pi após reiniciar com a imagem atualizada.

**Pendência do teclado USB no Pi físico.** No Raspberry Pi 3B+ a enumeração
para na primeira transferência de controle (`usb-device-desc8`) e o teclado
não funciona, embora o QEMU enumere normalmente. A tela do Pi mostra
`usb 21405 sp 1`, `hc 0 c c0000008`, `cfg 20002400 h 200000` e `fr 4 to 40`:
porta habilitada, full-speed, clock de SOF ativo e canal preso em
`CHENA|CHDIS` com `HCINT` zero. Foram testados e refutados PHY ULPI, PHY
UTMI, espera extra, `halt_channel`, o bit `MC`, o alias de barramento no
`HCDMA` e a ordem USB antes do vídeo. O mesmo driver funcionou no Pi no
Marco 13, antes da GUI. O detalhe está registrado em `agent.md`.

### Marco 24 — alteração de resolução segura

`video::init_mode(mode)` negocia, remapeia e publica um novo modo. Se falhar,
tenta renegociar o anterior; se nem isso funcionar, marca o vídeo como
indisponível em vez de deixar a tela apontando para buffer inválido.

O comando `resolucao` controla isso:

```text
rasppios> resolucao
resolucao: atual 640x480 stride 1280
rasppios> resolucao 1
resolucao: 800x600 aplicada; digite qualquer linha para confirmar ou 'cancela' para voltar
rasppios> ok
resolucao: confirmada 800x600
rasppios> resolucao
resolucao: atual 800x600 stride 1600
```

Regras:

- `resolucao` sem argumento mostra o modo ativo (dimensões e stride).
- `resolucao N` aplica o modo de índice `N` da tabela do Marco 23.
- A linha seguinte **confirma**, qualquer que seja o texto.
- `cancela` restaura o modo anterior.
- Sem resposta em `CONFIRM_TICKS` (1,5 s a 100 Hz), o modo anterior volta
  sozinho — cobre o caso em que o monitor não sincroniza.

`is_supported()` usa uma máscara calculada em tempo de compilação a partir da
tabela, sem consultar o firmware. Antes, ela lia um cache que só
`discover()` — chamado por `modos` — preenchia, então `resolucao N` podia
responder `modo nao suportado` logo após o boot. O bug foi observado no Pi e
está coberto por um ciclo red-green no `make check`.

A GUI agora calcula o layout a partir das dimensões reais
(`Layout::for_screen`), com painéis proporcionais e limites de linha por
painel. Em 640x480 o resultado é idêntico ao anterior; em 800x600 os painéis
crescem junto e a linha `video` do status acompanha a resolução.

`make check` cobre a troca real: aplica 800x600, confirma, verifica o modo
ativo, aplica 640x480, cancela e confirma a volta a 800x600. Também percorre
1024x768, 1280x720 (HD), 1280x1024 e 1920x1080 (Full HD), confirmando cada um.

### Marco 25 — mapa de endereços do framebuffer e modos altos

HD e Full HD falhavam porque a janela do programa de usuário estava em
`0x3C200000`, na mesma faixa em que o firmware aloca framebuffers maiores que
800x600 (`0x3C000000..0x3C280000`). O buffer novo caía sobre a página de
código do EL0 e o mapeamento era recusado.

Solução:

- Janela do EL0 movida para `0x3D000000`, no vão livre entre a RAM e o MMIO.
- Asserções em tempo de compilação (`USER_L2 == 488` e
  `USER_CODE_VA >= 0x3D00_0000`) impedem o retorno à faixa do framebuffer.
- `init_mode` libera o buffer anterior com `RELEASE_FRAMEBUFFER` antes de
  pedir outro.
- Conflito com a janela EL0 passa a ser diagnosticado na UART em vez de
  falhar em silêncio.

Modos validados no QEMU:

| Índice | Modo | Stride | Resultado |
|---|---|---|---|
| 0 | 640x480 | 1280 | ok |
| 1 | 800x600 | 1600 | ok |
| 2 | 1024x768 | 2048 | ok |
| 3 | 1280x720 (HD) | 2560 | ok |
| 4 | 1280x1024 | 2560 | ok |
| 5 | 1920x1080 (Full HD) | 3840 | ok |

As regressões do EL0 continuam passando: `usuario: el0`, `proprio: ok`,
`usuario: kernel negado`, `roda` com `app: ok`.

### Resolução padrão no boot

O kernel tenta, nesta ordem:

1. `1920x1080` (Full HD)
2. `1280x720` (HD)
3. `640x480` (modo seguro)

Se o firmware recusar um modo, a UART mostra
`video: 1920x1080 recusado; tentando o proximo` e o kernel segue para o
próximo. No QEMU o boot já entrega Full HD:

```text
video: 1920x1080 rgb565 stride 3840
```

O fallback foi comprovado forçando dois modos inválidos:

```text
video: 3840x2160 recusado; tentando o proximo
video: 2560x1440 recusado; tentando o proximo
video: 640x480 rgb565 stride 1280
```

### Confirmação de resolução

`resolucao N` aplica o modo e dá **10 segundos** para confirmar:

```text
rasppios> resolucao 3
resolucao: 1280x720 aplicada; digite qualquer linha para confirmar ou 'cancela' para voltar (10 s)
```

- Qualquer linha confirma.
- `cancela` restaura o modo anterior imediatamente.
- Sem entrada, o modo anterior volta sozinho ao fim do prazo — mesmo que
  ninguém digite nada. Isso protege quem não consegue ver a tela.

O comando `tela` e a cena de teste acompanham a resolução atual; não há mais
coordenadas fixas de 640x480 na cena.

### Marco 25 — reconstrução do framebuffer após mudança

Cada troca de resolução deixa o buffer anterior para trás. Antes,
`map_framebuffer` só reescrevia os blocos que tocava, então as páginas do
framebuffer antigo ficavam mapeadas como memória normal privilegiada para
sempre.

Agora o mapeamento é explícito e reversível:

- `FB_START`/`FB_END` guardam o intervalo em vigor.
- `restore_previous()` devolve cada bloco ao descritor original antes de
  aplicar o novo intervalo.
- `unmap_framebuffer()` é chamado quando o vídeo fica indisponível, para não
  sobrar tradução órfã.

O `make check` exercita sete trocas consecutivas:

```text
1920x1080 → 640x480 → 1920x1080 → 640x480 → 1920x1080 → 640x480 → 1280x720
tela: ok 1280x720 rgb565
```

Sem artefatos, sem perder a GUI e sem acesso fora dos limites.

### Marco 21 — imagem de distribuição

- `make dist` gera `rasppios-dist.img`: tabela MBR com uma partição FAT32
  LBA marcada como inicializável, começando no setor 2048, com
  `bootcode.bin`, `start.elf`, `fixup.dat`, `config.txt` e `kernel8.img`.
- A imagem é criada sem `root` e sem loop device: o sistema de arquivos é
  escrito no offset da partição com `mkfs.vfat --offset` e `mtools`.
- O script confere o próprio resultado: verifica a partição inicializável
  (`type=c`, flag `bootable`) e a presença dos cinco arquivos antes de
  declarar sucesso.
- `make check` também gera a imagem em diretório temporário e compara o
  `kernel8.img` embutido com `boot/kernel8.img` byte a byte.
- O comando `versao` imprime a identificação da versão e o procedimento de
  recuperação: sem RPFS o boot segue, a UART responde, e `arquivos` recria
  os arquivos de exemplo.

Gravar a imagem num cartão (substitui todo o conteúdo):

```bash
make dist
sudo dd if=rasppios-dist.img of=/dev/sdX bs=4M conv=fsync status=progress
sync
```

O caminho do dia a dia continua sendo `make write`, que copia os arquivos
para um cartão FAT32 já montado sem reformatar.

### Marco 22 — contrato gráfico e arquitetura

O contrato está em `src/gfx.rs` como código verificável, não só como
documento. Ele fixa o que o driver, o serviço gráfico e os clientes podem
combinar:

- **Camadas:** kernel (framebuffer, MMU, timer), serviço gráfico (compositor,
  superfícies, foco, filas) e aplicação (pede janela, desenha no próprio
  buffer, recebe eventos). Um cliente nunca recebe o endereço do framebuffer.
- **Formato:** `Rgb565`, 2 bytes por pixel.
- **Limites:** área de até 1920x1080, 8 superfícies, 8 janelas, título de até
  32 bytes, 16 mensagens na fila.
- **Modo:** `Mode::validate` rejeita largura/altura zero, modo grande demais,
  stride curto e stride não alinhado ao tamanho do pixel.
- **Geometria:** `Rect` com `intersect`, `contains` e `is_empty`.
- **Superfície:** `Surface::validate` confere recorte, stride e se
  `offset + length` cabe na arena do serviço.
- **API:** `create`, `destroy`, `move`, `resize`, `raise`, `damage`, `title`,
  `close`; eventos `key`, `pointer`, `focus`, `close`.

O comando `gfx` roda o autoteste e imprime `gfx: contrato ok` ou a contagem de
falhas; `make check` exige a linha de sucesso. O autoteste pegou dois defeitos
reais no contrato: `create` valida largura e altura em `c,d`, e `Message`
ocupa 20 bytes, não 24.

```text
rasppios> gfx
gfx modo 640x480: ok
gfx superficie fora da arena: ok
gfx msg criar sem area: ok
gfx: contrato ok
```

### Marco 23 — descoberta e seleção de modo de vídeo

`src/modes.rs` pergunta ao firmware, pelo mailbox de propriedades, quais são
as dimensões e o stride correntes, e mantém uma tabela conservadora de seis
modos RGB565:

| Índice | Modo | Stride | Buffer |
|---|---|---|---|
| 0 | 640x480 | 1280 | 614 400 |
| 1 | 800x600 | 1600 | 960 000 |
| 2 | 1024x768 | 2048 | 1 572 864 |
| 3 | 1280x720 | 2560 | 1 843 200 |
| 4 | 1280x1024 | 2560 | 2 621 440 |
| 5 | 1920x1080 | 3840 | 4 147 200 |

O índice 0 é o **modo seguro**: a menor área, usada como destino de
recuperação. Todo modo passa pelo contrato de `gfx` antes de ser aceito, e um
índice fora da tabela é rejeitado.

O comando `modos` imprime o modo atual, quantos da tabela são aceitos e cada
modo com `stride` e tamanho de buffer, marcando com `*` o que está ativo:

```text
rasppios> modos
modos: atual 640x480 stride 1280
modos: 6 aceitos de 6
modo[0]* 640x480 stride 1280 buffer 614400
modo[1] 800x600 stride 1600 buffer 960000
...
modos: descoberta ok
```

`make check` exige a linha `modos: descoberta ok`.

Dois detalhes de contrato do mailbox que o teste pegou: a tag de leitura é
`0x0004_0003` (`GET_PHYSICAL_WIDTH_HEIGHT`) — `0x0004_8003` é a de escrita — e
o código de resposta fica na palavra 4 do buffer, não na palavra 3, que é o
tamanho da tag.

## Compilar e testar no QEMU

```bash
make check
```

Saída esperada de `make check` (sobe `boot/kernel8.img`, o mesmo arquivo do cartão):

```
rasppios: marco 21
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

Depois de gravar, desmonte a unidade antes de retirar o cartão:

```bash
sync
udisksctl unmount -b /dev/sda1
findmnt -n /media/vladimir/PENDRIVE || echo "desmontado"
```

`make image` gera `rasppios.img`, uma imagem de teste exFAT de 64 MiB com os
mesmos cinco arquivos, conforme `scripts/image.sh`. O firmware do Pi exige
FAT32 no cartão usado para o boot; `make write` copia os arquivos para a mídia
montada sem formatá-la. Quando encontra o volume, o firmware carrega o kernel
em `0x80000`.

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

- `rasppios-etapa` — atualizar `agent.md` e este arquivo, rodar `make check` e copiar para o cartão FAT32.
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
