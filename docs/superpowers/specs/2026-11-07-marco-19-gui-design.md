# Marco 19 — interface gráfica inicial

## Objetivo

Adicionar uma GUI mínima que coexista com o console UART/USB já existente:

- compositor direto sobre o framebuffer RGB565 do marco 18;
- barra superior e três janelas retangulares fixas;
- foco de janela controlado pelo teclado USB;
- uma janela de terminal gráfico que mostra comandos e respostas básicos;
- UART continua funcionando como console independente.

O primeiro marco gráfico não terá mouse, arrastar janelas, processos gráficos
ou substituição do shell serial.

## Arquitetura escolhida

O compositor fará redraw completo da cena a cada atualização. Essa escolha
mantém o estado visual determinístico e evita introduzir, neste marco, uma
infraestrutura de regiões sujas ou superfícies por processo.

Será criado um módulo `src/gui.rs` com estado exclusivo do núcleo 0:

- dimensões e paleta da GUI;
- janela focada;
- texto visível do terminal;
- estado do teclado e fila mínima de eventos;
- funções de layout, desenho e atualização.

O módulo reutilizará as primitivas seguras de `video.rs` (`clear`, `rect`,
`text` e o flush da cena) e não escreverá diretamente em endereços de vídeo.
A entrada USB continuará sendo obtida pelo mecanismo de consulta existente;
quando a GUI estiver ativa, ela consumirá somente eventos destinados à GUI e
deixará a UART para o console serial.

## Layout e aparência

A cena será 640×480, RGB565, conforme o framebuffer negociado no marco 18:

- fundo azul-escuro;
- barra superior de 24 pixels com o título `rasppios gui`;
- janela esquerda `status`, 200×180;
- janela direita superior `terminal`, 392×180;
- janela inferior `eventos`, ocupando a largura restante;
- borda destacada e título diferente para a janela focada.

O terminal gráfico terá fundo preto, texto branco e uma linha inicial
`rasppios gui>`. O conteúdo inicial de status mostrará `EL1`, `video 640x480`
e `UART ativa`. A área de eventos mostrará o foco atual e as últimas teclas
reconhecidas.

## Entrada e foco

- `Tab` alterna o foco entre `status`, `terminal` e `eventos`;
- caracteres imprimíveis recebidos pelo teclado entram no terminal quando ele
  está focado;
- `Backspace` remove o último caractere do terminal;
- `Enter` registra a linha no terminal e mostra uma resposta local para
  `help`, `clear` e `echo texto`;
- outras linhas recebem `gui: comando desconhecido`;
- UART continua alimentando `console::run` e não perde seus comandos.

Como o driver atual entrega caracteres já traduzidos ao console, a integração
deve preservar esse contrato e adicionar apenas os eventos necessários para
Tab, Backspace e Enter. Se um evento não puder ser distinguido com segurança,
ele não deve alterar o foco nem escrever fora da área do terminal.

## Ciclo de atualização

O boot inicializará a GUI depois de o framebuffer estar disponível e depois
publicará os núcleos secundários. A GUI desenhará a cena inicial uma vez. O
loop do núcleo 0 continuará atendendo UART, USB e o timer; quando houver
mudança de foco ou texto, fará redraw completo e flush do framebuffer.

O console serial continuará sendo o caminho de diagnóstico e de recuperação:
se a GUI falhar ao inicializar, o kernel imprimirá `gui: indisponivel` e
entrará no shell existente sem tocar no framebuffer inválido.

## Erros e invariantes

- cada retângulo e texto será recortado pelas dimensões atuais do vídeo;
- o estado do terminal terá limite fixo e não poderá crescer sem limite;
- o redraw não poderá escrever fora do framebuffer negociado;
- uma entrada USB inválida será ignorada;
- a GUI não mudará permissões da MMU nem exporá framebuffer ao EL0;
- o panic handler e a UART continuarão disponíveis se a GUI falhar.

## Aceite

### QEMU

Com a janela gráfica e `-device usb-kbd`:

1. a tela inicial mostra barra, três janelas e terminal;
2. `Tab` altera a borda da janela focada;
3. o terminal aceita `echo oi` e mostra `oi`;
4. `Backspace` e `Enter` funcionam;
5. a aba `serial1` continua aceitando `help`, `ls` e `tela`;
6. o `make check` mantém todos os testes dos marcos 1–18.

### Raspberry Pi 3B+

No monitor físico, a mesma tela aparece durante a carga; um teclado USB
altera o foco e o terminal mostra `echo oi`. A UART continua mostrando o
banner e aceitando o shell. O marco só será marcado como feito no Pi depois
dessas observações.
