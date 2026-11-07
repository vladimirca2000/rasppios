# Plano de desenvolvimento do sistema operacional para Raspberry Pi 3B+

Precisamos de um plano de etapas testável, incremental e fiel à filosofia do Tanenbaum. Vamos dividir em **fases que produzem algo executável a cada etapa** — primeiro em VM (QEMU), depois no Raspberry Pi 3B+.

## Como conduzir cada etapa

Skills do repositório, lidas antes de codar: `.cursor/skills/rasppios-etapa` (fluxo do marco) e `.cursor/skills/rasppios-bcm2837` (mapa do Pi 3B+ e do QEMU). Rust `no_std`, assembly, linker, MMIO e QEMU genérico continuam nas skills pessoais já instaladas.

Sempre que o código, a imagem ou uma decisão técnica mudar:

1. Atualizar este arquivo: o que mudou, em qual marco, e o que ainda falta para o aceite.
2. Atualizar o `README.md` com o que já está feito e como reproduzir.
3. No fim de cada marco concluído no QEMU, nesta ordem:
   - Rodar `make check`.
   - Abrir o QEMU com janela para visualizar a miniUART. A aba `bcm2835-fb` fica preta até o marco de vídeo; o texto está na aba `serial1`.

```bash
qemu-system-aarch64 -M raspi3b -kernel boot/kernel8.img \
  -serial null -serial vc:100Cx30C \
  -monitor none -display gtk,show-tabs=on,zoom-to-fit=on -no-reboot
```

   - Gravar a imagem no pendrive já montado, sem formatar:

```bash
make write DEST=/media/vladimir/PENDRIVE
```

Confirmar com `findmnt -n -o SOURCE,FSTYPE,TARGET /media/vladimir/PENDRIVE` que o destino é `/dev/sda1 vfat /media/vladimir/PENDRIVE` e que a origem não é `nvme`. A cópia recusa outro sistema de arquivos e recusa os NVMe. Não usar `scripts/write-drive.sh`. Se o cartão não estiver montado, não inventar outro destino.

## Estado atual

**Marco 1 no QEMU: feito. No Pi: feito.** A imagem sobe e a miniUART imprime o banner. Em 2026-11-06 o cartão FAT32 iniciou o Pi 3B+ e a UART física mostrou o banner com `exception level: EL1`.

**Marco 2 no QEMU: feito. No Pi: feito.** Cada núcleo que entra em `_start` recebe 32 KiB de pilha e é identificado por `MPIDR_EL1`. Só o núcleo 0 chama `rust_main`. No `raspi3b` do QEMU 8.2 os núcleos 1–3 não recebem o endereço do kernel, então a linha de chegada é `0=1 1=0 2=0 3=0`. O firmware do Pi segura os secundários fora da imagem. Em 2026-11-06 a UART física mostrou a mesma linha, `kernel somente no nucleo 0`, e o boot chegou ao prompt.

**Marco 3 no QEMU: feito. No Pi: feito.** A miniUART recebe pela FIFO (`AUX_MU_LSR` bit 0, página 15 do BCM2835) e o núcleo 0 fica num console. O prompt `rasppios>` ecoa a linha e responde a `help`, `el`, `nucleo` e `eco`. `make check` sobe a imagem duas vezes e, num PTY, envia esses comandos depois do prompt; a resposta de `el` é `el: EL1`, o nível em que o kernel fica depois de descer. A formatação das mensagens passa por `core::fmt::Write`. Em 2026-11-06 a UART física aceitou `usuario` e voltou ao prompt. A espera do caractere ainda é por consulta; interrupção é o marco 5.

**Marco 4 no QEMU: feito. No Pi: aberto.** `VBAR_ELx` aponta para uma tabela alinhada em 2 KiB. `SPSel` fica em 1, então a exceção síncrona deste nível entra no offset `0x200`. O comando `falha` executa `BRK #1` com `x0 = 0x41`. O relato mostra `causa: brk`, `esr: 0xf2000001` (classe 0x3C e bit IL) e os registradores `elr`, `spsr`, `far`, `sp` e `x0`, depois volta ao prompt. O comando `panico` imprime `PANIC`, o arquivo e a mensagem `provocado`, e para sem reiniciar. Falta o mesmo relato na UART física.

**Marco 5 no QEMU: feito. No Pi: aberto.** O system timer usa o canal 1 (`ST_C1`, IRQ 1 do pending 1). O `CLO` no QEMU 8.2 anda a 1 MHz e o compare dispara a cada 10 000 contas, 100 Hz. `HCR_EL2.IMO` desvia a IRQ para EL2 e `DAIF.I` é liberado. O comando `ticks` imprime o contador e o `CLO`; em meio segundo o `make check` viu o contador subir 50, de `clo: 4606` para `clo: 505089`. Em 2026-11-06 a janela do QEMU foi aberta na segunda tela e `kernel8.img` foi copiado para `/media/vladimir/PENDRIVE` (`/dev/sda1`, FAT32), idêntico ao que o `make check` executou. A aba `bcm2835-fb` continua preta; o texto fica na aba `serial1`. Falta ver o contador avançar na UART física.

**Marco 6 no QEMU: feito. No Pi: aberto.** O canal 8 do mailbox pergunta `GET_ARM_MEMORY`. No `raspi3b` do QEMU 8.2 a resposta é base 0 e `0x3C000000` bytes: 1 GiB menos os 64 MiB do VideoCore. O heap começa depois de `__stacks_end` e para nesse limite, antes do MMIO em `0x3F000000`. O comando `mem` aloca dois blocos de 4096, libera o primeiro e o recebe de volta no mesmo endereço. Em 2026-11-06 a janela do QEMU mostrou esse relato na aba `serial1`, e `kernel8.img` foi copiado para `/media/vladimir/PENDRIVE` (`/dev/sda1`, FAT32). Falta o mesmo mapa na UART física; se o firmware do Pi reservar outro tamanho de GPU, o mailbox é quem deve prevalecer.

**Marco 7 no QEMU: feito. No Pi: aberto.** A MMU de estágio 1 faz identidade. Os primeiros 2 MiB usam páginas de 4 KiB: código executável e só leitura, constantes só leitura, dados graváveis e não executáveis. O resto da RAM até `0x3C000000` são blocos de 2 MiB. O MMIO `0x3F000000..0x40000000` é device-nGnRnE. `0x3E000000` não tem descritor. `pagina` grava e lê `0xa5a5a5a5a5a5a5a5` numa página mapeada; o acesso ao buraco gera abort de dados com `esr: 0x96000006` (classe 0x25, falha de tradução no nível 2) e `far: 0x000000003e000000`, e o console continua. O timer e a miniUART seguem depois da MMU ligada. Em 2026-11-06 a janela do QEMU mostrou esse relato na aba `serial1`. Falta o mesmo teste na UART física.

**Marco 8 no QEMU: feito. No Pi: aberto.** O núcleo 0 grava `_start` no spin table (`0xe0`, `0xe8`, `0xf0`), limpa a linha de cache e manda `sev`. Os núcleos 1–3 ligam a mesma MMU e entram na trava. Cada um dos quatro soma 1000 com carga e armazenamento separados. `trava` imprimiu `nucleos 4 soma 4000`. A pilha de cada núcleo cabe nos 128 KiB reservados: o núcleo N usa `[__stacks_end - (N + 1) * 32 KiB, __stacks_end - N * 32 KiB)`. Em 2026-11-06 a janela do QEMU mostrou esse relato na aba `serial1`. Falta o mesmo teste na UART física do Pi, com os quatro núcleos de verdade.

**Marco 9 no QEMU: feito. No Pi: aberto.** Duas tarefas de kernel alternam por `yield`, cada uma com pilha de 4 KiB. A troca guarda `x19`–`x28`, `x29`, `x30` e `sp`. `tarefas` rodou `a` e `b` quatro vezes cada, na ordem `abababab`, e o valor privado de cada uma sobreviveu às trocas (`preservado: ok`). Em 2026-11-06 a janela do QEMU mostrou esse relato na aba `serial1`. Falta o mesmo teste na UART física, estável por um período no Pi.

**Marco 10 no QEMU: feito. No Pi: aberto.** A IRQ do timer, a 100 Hz, troca a tarefa corrente se a preempção está ligada. A escolha é a maior prioridade; empate alterna. Uma tarefa de prioridade 2 corre antes das outras, que já estavam prontas. As duas de prioridade 1 não chamam `yield` na espera: cada uma só avança quando o timer entregou o núcleo à outra. `preempcao` imprimiu `alta antes` e `fatias: abababab`. O console e a UART seguem depois. Em 2026-11-06 `kernel8.img` foi copiado para `/media/vladimir/PENDRIVE` (`/dev/sda1`, FAT32), idêntico ao que o `make check` executou, e a janela do QEMU foi aberta. A aba `bcm2835-fb` continua preta; o texto fica na aba `serial1`. Falta o mesmo teste na UART física, com carga concorrente, sem travar UART nem timer.

**Marco 11 no QEMU: feito. No Pi: feito.** O firmware entrega EL2 e `make run` entrega EL3; `boot.s` desce a EL1 antes do banner, com `HCR_EL2.RW` e a IRQ física ficando em EL1. `CPACR_EL1.FPEN` libera SIMD só no EL1: o compilador zera o buffer da linha com `movi`, e o EL0 continua sem esses registradores. A MMU segue identidade para o kernel, com `AP[2:1]` privilegiado. O programa de EL0 mora numa janela à parte, `0x3C200000`, com código, dados e pilha em páginas de 4 KiB que apontam para molduras do kernel. `usuario` lê a própria página (`proprio: ok`) e a carga de `0x80000` gera abort de dados do nível inferior, `esr: 0x9200000f` (classe 0x24, falha de permissão no nível 3) e `far: 0x0000000000080000`. O vetor devolve o núcleo ao comando. No ELF os quatro núcleos entram juntos; os secundários esperam `MMU_READY` antes de ligar a MMU, e o banner segue inteiro com `chegada: 0=1 1=1 2=1 3=1`. Em 2026-11-06 a UART física do Pi 3B+ mostrou o mesmo relato, com `elr: 0x000000003c20001c`, `spsr: 0x00000000000003c0` e o prompt de volta. A tentativa inválida ficou contida.

**Marco 12 no QEMU: feito. No Pi: feito.** O EL0 pede serviço com `SVC` e o número em `x8`. A chamada 0 escreve na miniUART só se o intervalo cabe na janela `0x3C200000..0x3C203000`; um ponteiro em `0x80000` volta com `x0 = !0` e não é lido. A chamada 1 lê a miniUART para uma página gravável dessa janela. A chamada 2 devolve o `CLO`. A chamada 3 encerra e o vetor volta ao comando. O hardware já deixa o `ELR` na instrução seguinte ao `SVC`. No QEMU, `chamada` imprimiu `saida: ok`, `ponteiro: negado`, um `tempo` maior que o `CLO` visto em `ticks`, `entrada: Z` e `fim: 0`. Em 2026-11-06 a UART física do Pi 3B+ mostrou `saida: ok`, `ponteiro: negado`, `tempo: 131104175` e `fim: 0`, e o prompt voltou. O byte lido não tem glifo: a linha ficou `entrada:` vazia, compatível com o CR do Enter. As quatro chamadas responderam e o console continuou.

**Marco 13 no QEMU: feito. No Pi: feito.** O DWC2 em `0x3F980000` sobe em modo host e transfere por DMA. A configuração cabe um hub de um nível e um teclado HID de protocolo de boot. No `raspi3b` o QEMU põe um hub full-speed na porta e o `usb-kbd` atrás dele; o teste manda `eco usb` por `sendkey` e a miniUART ecoa a linha. Se a porta raiz for high-speed e o teclado não, a transação usa split. Sem teclado a linha é `teclado: ausente` e o console serial segue. Em 2026-11-06 a UART física do Pi 3B+ mostrou `teclado: hid`, a linha digitada `eco usb` e a resposta `eco: usb`, com o prompt de volta.

**Marco 14 no QEMU: feito. No Pi: feito.** O microSD do Pi 3B+ fica no SDHOST (`0x3F202000`). No QEMU o cartão nasce no Arasan; a função ALT0 nos GPIO 48–53, a mesma que o firmware já usa no Pi, entrega o cartão ao SDHOST. `bloco` lê o bloco 0, grava `0xA5` no bloco 1, lê de volta e restaura os bytes originais. Na imagem zerada de 4 MiB o teste viu `leitura: 00000000`, `escrita: ok` e `restauro: ok`. Em 2026-11-06 a UART física do Pi 3B+ mostrou as mesmas três linhas, com o teclado ainda em `teclado: hid` e o prompt de volta. O bloco 1 foi restaurado.

**Marco 15 no QEMU: feito. No Pi: aberto.** O sistema de arquivos `RPFS` ocupa os blocos 64 em diante: superbloco, raiz, mapa de blocos e dados. Cada arquivo cabe em 512 bytes. `arquivos` formata essa região se a marca não existe, cria o arquivo `ola` e o diretório `notas` com o arquivo `item`, lista e lê. Um segundo boot da mesma imagem imprime `montagem: pronta` e `persistiu: ok`. Os blocos 0 e 1 ficam fora dessa região. Em 2026-11-06 `kernel8.img` foi copiado para `/media/vladimir/PENDRIVE` (`/dev/sda1`, FAT32), idêntico ao que o `make check` executou, e a janela do QEMU foi aberta. A aba `bcm2835-fb` continua preta; o texto fica na aba `serial1`. Em 2026-11-06 a UART física mostrou a primeira execução: `montagem: nova`, `lista: ola notas`, `leitura: rasppios` e `sub: dentro`. Falta reiniciar o Pi e mandar `arquivos` de novo: a segunda vez tem de mostrar `montagem: pronta` e `persistiu: ok`, lendo `rasppios` e `dentro` sem reformatar.

**Marco 16 no QEMU: feito. No Pi: feito.** O arquivo `prog` no `RPFS` é uma imagem `RPEX`: marca, versão 1, entrada no início do código e o tamanho. O carregador lê esse arquivo do cartão, copia o código para `0x3C200000` e entra em EL0. A aplicação escreve `app: ok` pela chamada 0 e termina com a chamada 3, status 7. A primeira execução grava o arquivo (`roda: gravou`). A seguinte, no QEMU depois de reiniciar e no Pi na mesma sessão, só lê o que já está no disco (`roda: disco`) e devolve o mesmo status. Em 2026-11-06 a UART física mostrou as duas linhas: `roda: gravou` e `roda: disco`, ambas com `app: ok` e `saida: 7`.

**Marco 17 no QEMU: feito. No Pi: aberto.** O console responde a `ls`, `cat`, `echo` e `run`, além dos diagnósticos que já existiam. `eco` continua valendo o mesmo que `echo`. `ls` lista a raiz ou um diretório, `cat` lê um arquivo, inclusive `notas/item`, e `run prog` carrega a imagem `RPEX` que já está no cartão. A sequência automatizada viu `ls: ola notas prog`, `ls: item`, `cat: rasppios`, `cat: dentro`, `echo: shell` e `run: ok 7`. O teclado USB entra na mesma linha do serial. A escrita agora espera a FSM do SDHOST e o CMD13 até o cartão voltar ao estado `tran`, e a FIFO fica no limiar 4 do defeito de silício. Se `roda` formatar o cartão antes, `arquivos` completa `ola` e `notas` sem apagar o `prog`. O nome curto `arquivo` chama o mesmo comando. O cartão foi reformatado em FAT32 e recebeu novamente os arquivos de boot; isso apagou o RPFS. Falta agora, no Pi, executar `arquivos`, `ls`, `ls notas`, `cat ola`, `cat notas/item`, `echo shell` e `run prog`, e enviar a UART para fechar o marco.

**Marco 18 no QEMU: feito. No Pi: aberto.** O framebuffer negociado no QEMU `raspi3b` é `640x480`, RGB565, com `stride 1280`. A cena fixa tem fundo azul-escuro, faixa azul, três retângulos e os textos `rasppios marco 18`, `framebuffer: 640x480 rgb565` e `UART continua ativa`; a UART continua na aba `serial1`. A reprodução visual usa:

```bash
qemu-system-aarch64 -M raspi3b -kernel boot/kernel8.img \
  -drive if=sd,format=raw,file=/tmp/rasppios-sd.img \
  -device usb-kbd -serial null -serial vc:100Cx30C \
  -monitor none -display gtk,show-tabs=on,zoom-to-fit=on -no-reboot
```

Na aba `serial1`, `tela` responde `tela: ok 640x480 rgb565`; na aba `bcm2835-fb`, a cena foi confirmada. Ainda não há evidência física do monitor no Pi 3B+, portanto o aceite de vídeo no hardware permanece aberto.

Mídia de teste: `/dev/sda` de 29,7 GB, um cartão SD num adaptador USB, partição `/dev/sda1` montada em `/media/vladimir/PENDRIVE`. Antes dos testes, uma única conversão exFAT→FAT32 foi feita nessa partição; desde então, o cartão permanece em FAT32, tipo `W95 FAT32 (LBA)`, rótulo `PENDRIVE`. O `make write` copia os cinco arquivos de boot sem formatar e, no procedimento atual, o destino esperado é `/dev/sda1` em FAT32.

A documentação do boot trata a partição de carregamento como FAT/FAT32 ([hardware do Raspberry Pi](https://www.raspberrypi.com/documentation/computers/raspberry-pi.html), [Compute Module, primeira partição FAT](https://www.raspberrypi.com/documentation/computers/compute-module.html)). O fórum oficial registra que exFAT não inicia o Pi ([tópico 186215](https://forums.raspberrypi.com/viewtopic.php?t=186215)).

Referências usadas no bring-up: [config.txt](https://www.raspberrypi.com/documentation/computers/config_txt.html), [processadores, BCM2837B0](https://www.raspberrypi.com/documentation/computers/processors.html), [placas clássicas, Pi 3B+](https://www.raspberrypi.com/documentation/computers/sbcs.html), [BCM2835 ARM Peripherals](https://www.raspberrypi.org/app/uploads/2012/02/BCM2835-ARM-Peripherals.pdf). O `kernel8.img` é carregado em `0x80000` ([firmware #1816](https://github.com/raspberrypi/firmware/issues/1816)).

## O que conta como sistema operacional?

Não há uma fronteira universal e precisa: a definição depende do escopo. Em termos práticos, um **kernel que inicia e imprime uma mensagem é um núcleo experimental**, mas ainda não é um sistema utilizável. Um SO começa a cumprir seu papel quando gerencia recursos e oferece serviços aos programas; para este projeto, isso significa, no mínimo, processos, memória protegida, chamadas de sistema, entrada/saída e armazenamento. A definição funcional de SO da OpenStax enfatiza justamente gestão de recursos, isolamento, acesso controlado ao hardware e serviços para aplicações. ([OpenStax — What Is an Operating System?](https://openstax.org/books/introduction-computer-science/pages/6-1-what-is-an-operating-system))

A 5ª edição de Tanenbaum e Bos organiza os temas em processos e threads, memória, sistemas de arquivos, E/S, deadlocks, virtualização, multiprocessadores, segurança e projeto de SO. Isso é um **mapa de conceitos**, não uma exigência de implementar todos os recursos avançados antes de ter um sistema útil. ([Pearson — Modern Operating Systems, 5th edition](https://www.pearson.com/en-us/subject-catalog/p/modern-operating-systems/P200000003295/9780137618880))

## Plano incremental de entregas

Cada entrega abaixo deve ser integrada às anteriores. **“Teste no QEMU” não substitui o teste no Raspberry Pi**; os dois fazem parte do critério de aceite.

| Marco | Entrega verificável | Aceite no QEMU | Aceite no Raspberry Pi 3B+ |
|---|---|---|---|
| **1. Projeto inicializável** | Repositório, compilação cruzada AArch64, imagem mínima e configuração de boot documentada | A imagem inicia no modelo `raspi3b` do QEMU | O cartão de teste inicia a imagem e produz saída na UART |
| **2. Inicialização multicore segura** | Assembly de entrada, pilha, identificação do núcleo e estacionamento dos núcleos secundários | Inicialização repetível; apenas o núcleo escolhido executa o kernel | Mesmo comportamento observado pela UART, sem travamentos de boot |
| **3. Console serial** | Driver UART, formatação de mensagens e comandos de diagnóstico | Mensagens e caracteres recebidos funcionam | Console permite acompanhar boot e enviar entrada serial |
| **4. Exceções e falhas** | Vetores AArch64, relatório de exceções e panic handler | Exceções provocadas aparecem com causa e registradores úteis | Erro controlado gera diagnóstico na UART, sem reinício imprevisível |
| **5. Temporizador e interrupções** | Timer, interrupções habilitadas e contagem de ticks | Interrupções periódicas ocorrem na frequência esperada | Timer real funciona; contadores avançam sem depender do QEMU |
| **6. Alocação de memória física** | Mapa de memória, reserva de regiões e alocador simples | Testes de alocar/liberar passam; sem sobreposição de blocos | Mapa reconhece a RAM disponível e os testes básicos passam |
| **7. MMU e memória virtual** | Tabelas de páginas, tradução e mapeamentos do kernel | Leitura/escrita em páginas mapeadas funciona; página inválida gera exceção | Kernel inicia com MMU ativa e acesso a periféricos continua correto |
| **8. Primitivas de concorrência** | Locks, exclusão mútua e sincronização entre núcleos | Testes concorrentes não perdem atualizações | Testes nos núcleos do Pi passam sem deadlock ou corrupção |
| **9. Primeiro escalonador** | Threads de kernel e troca cooperativa de contexto | Duas tarefas alternam e preservam estado | Alternância ocorre no hardware e continua estável por um período definido |
| **10. Preempção e prioridade** | Escalonador preemptivo com prioridades simples | Timer interrompe uma tarefa e dá execução à outra | Preempção funciona com carga concorrente, sem travar UART/timer |
| **11. Modo de usuário e isolamento** | Separação EL0/EL1, espaços de endereçamento e retorno seguro ao kernel | Programa em EL0 não consegue acessar memória do kernel | Teste equivalente no Pi; tentativa inválida é contida, não derruba o sistema inteiro |
| **12. Chamadas de sistema** | Interface inicial: saída, encerramento, consulta de tempo e leitura de entrada | Programas de teste usam chamadas e recebem resultados esperados | Os mesmos programas executam no Pi e usam a interface do kernel |
| **13. Entrada de teclado** | Driver USB/HID, primeiro em uma configuração limitada e documentada | Teclado virtual compatível envia eventos ao sistema | Teclado USB conectado ao Pi envia caracteres ao console/shell |
| **14. Armazenamento em blocos** | Leitura e escrita de blocos no cartão SD | Imagem de disco de teste pode ser lida e gravada | O cartão de teste é acessado pelo driver próprio, sem depender de Linux |
| **15. Sistema de arquivos mínimo** | Montagem, diretórios e operações básicas; começar com escopo pequeno | Testes automatizados de criar, listar, ler e gravar arquivos | Os mesmos testes passam no SD real; reiniciar preserva os dados |
| **16. Carregador de programas** | Formato executável definido, carregamento e início de aplicações em EL0 | Aplicação de teste inicia, termina e devolve código de saída | Aplicação equivalente executa a partir do armazenamento do Pi |
| **17. Shell útil** | Comandos como `help`, `ls`, `cat`, `echo`, `run` e diagnóstico | Sequência de comandos pode ser automatizada e comparada com resultados esperados | Shell opera pelo console serial; teclado USB pode ser acrescentado como entrada |
| **18. Saída gráfica** | Framebuffer, desenho de pixels, texto e fonte bitmap | Tela de teste e renderização de texto são verificáveis | Imagem aparece pela saída de vídeo do Pi; resolução e modo são registrados |
| **19. Interface gráfica inicial** | Compositor simples, janelas básicas, foco e terminal gráfico | Teste visual e interação por entrada emulada, quando suportada | Interface aparece no monitor e responde a teclado e mouse USB |
| **20. Robustez e recuperação** | Logs, limites, tratamento de erros, testes de carga e consistência do sistema de arquivos | Testes prolongados, falhas provocadas e regressão automatizada | Testes prolongados no Pi, incluindo reinício e recuperação após falhas controladas |
| **21. Imagem de distribuição do projeto** | Cartão inicializável, instruções de instalação, versão e procedimento de recuperação | A imagem final inicia limpa no modelo de teste | Cartão preparado do zero inicia o sistema no Pi e chega ao shell/ambiente previsto |

Para manter cada etapa pequena, alguns marcos podem ser divididos em tarefas menores dentro do repositório. Por exemplo, o marco de sistema de arquivos pode virar “montar”, “listar”, “ler”, “gravar” e “testar corrupção”, cada qual com seu próprio teste.

## Quando teremos um SO funcional e quando será viável?

- **Núcleo experimental:** marcos 1–7. O código inicializa e começa a gerir hardware e memória, mas ainda não oferece um ambiente geral para programas.
- **SO funcional mínimo:** aproximadamente marcos 11–17. Há isolamento entre kernel e aplicações, chamadas de sistema, execução de programas, armazenamento e shell.
- **SO viável para o objetivo do projeto:** marcos 18–21 também concluídos, com interface gráfica, periféricos necessários, testes de estabilidade e uma forma reproduzível de preparar o cartão. “Viável” aqui é um critério definido para este projeto, não uma certificação geral.

Para respeitar os princípios do livro sem tornar o começo grande demais, eu usaria **separação clara entre subsistemas, abstrações e interfaces estáveis** desde o início. Uma implementação inicialmente modular e simples pode permitir reorganizar componentes depois; não é necessário decidir agora por um microkernel completo para estudar e aplicar os conceitos.

## Pontos técnicos importantes para este alvo

O Raspberry Pi 3B+ tem Cortex‑A53 ARMv8 de 64 bits e 1 GB de RAM. ([Raspberry Pi — especificações do 3B+](https://www.raspberrypi.com/products/raspberry-pi-3-model-b-plus)) O Cortex‑A53 suporta os níveis de exceção ARMv8; separar aplicações em EL0 do kernel em EL1 será essencial para proteção, chamadas de sistema e falhas isoladas. ([Arm — switching exception levels](https://learn.arm.com/learning-paths/embedded-and-microcontrollers/bare-metal/exception-levels))

Há uma ressalva sobre “VM”: o QEMU oferece `raspi3b`, não um modelo separado identificado como `raspi3b+`. Ele emula CPU, temporizador, UART, GPIO, framebuffer e outros dispositivos, mas não representa perfeitamente todas as diferenças do hardware real. Portanto, use o QEMU para testes rápidos e automatizados, e o Pi real para validar boot, cartão SD, vídeo, USB e periféricos físicos. ([QEMU — modelos de Raspberry Pi e dispositivos emulados](https://www.qemu.org/docs/master/system/arm/raspi.html))

**Correção importante do plano anterior:** não devemos assumir um GIC genérico; o projeto deve seguir o controlador de interrupções e os periféricos reais do BCM2837/B+ e confirmar as diferenças no hardware. Também é mais seguro usar o firmware de boot existente do Raspberry Pi para carregar nosso kernel próprio, em vez de tentar escrever todo o bootloader como primeira etapa. O sistema continuará sendo novo e sem Linux; o firmware da placa é apenas a etapa inicial de carregamento.

Minha recomendação é declarar concluído cada marco só quando passar por três checagens: **teste automatizado, boot no QEMU e boot no Pi real**, mantendo a última versão funcional do cartão como recuperação.