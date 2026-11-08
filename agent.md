# Plano de desenvolvimento do sistema operacional para Raspberry Pi 3B+

Precisamos de um plano de etapas testável, incremental e fiel à filosofia do Tanenbaum. Vamos dividir em **fases que produzem algo executável a cada etapa** — primeiro em VM (QEMU), depois no Raspberry Pi 3B+.

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

---

# Complemento: ambiente gráfico de janelas e programas iniciais

Os marcos **1–21 são o plano-base** e, conforme informado, já foram concluídos. Este complemento parte desse ponto para transformar a saída gráfica e as janelas experimentais do marco 19 em um **ambiente gráfico de janelas utilizável**, com controle de resolução, múltiplos programas, shell em janela, painel de controle, explorador de arquivos, barra de tarefas e editor de texto.

Neste documento, “X” significa o ambiente gráfico de janelas do projeto. **Não implica implementar o protocolo X11 nem compatibilidade com Linux/X.Org.** A meta é um sistema gráfico próprio, pequeno e adequado ao Raspberry Pi 3B+, com API documentada para aplicações nativas.

## Regras de teste para todos os novos marcos

Cada marco deve incluir testes automatizados dos componentes, um roteiro reproduzível no QEMU e uma validação em Raspberry Pi real. O QEMU pode não emular fielmente o firmware de vídeo, a negociação de resolução, o USB ou cada monitor; nesses casos, ele valida a lógica e o rendering em framebuffer, enquanto o teste físico valida o comportamento do monitor e dos periféricos.

Para cada versão, guardar: identificação do commit/versão, configuração de tela usada, resultado dos testes, logs seriais e uma imagem/cartão anterior que ainda inicializa. Alterações de resolução e gravações em disco devem falhar de forma segura e não corromper o sistema.

## Novos marcos de desenvolvimento

| Marco | Entrega verificável | Aceite no QEMU | Aceite no Raspberry Pi 3B+ |
|---|---|---|---|
| **22. Contrato gráfico e arquitetura** | Especificação dos componentes: driver de vídeo, servidor/compositor, entrada, API de janelas e clientes; separação entre kernel, serviços e aplicações | Testes verificam dimensões, formatos de pixel, coordenadas, ownership dos buffers e mensagens da API | Boot gráfico continua funcionando e os mesmos contratos básicos são exercitados no hardware |
| **23. Descoberta e seleção de modo de vídeo** | Consulta dos modos disponíveis, resolução atual, profundidade/formato de pixel e limites de memória; lista segura de modos suportados | Testes com modos simulados incluem válidos, inválidos e falhas do serviço gráfico | Detecta os modos realmente oferecidos pelo firmware/monitor e registra resolução atual pela UART |
| **24. Alteração de resolução segura** | Serviço para solicitar e aplicar uma resolução suportada; validação de dimensões/stride/tamanho do buffer, liberação do buffer anterior e mecanismo de retorno ao modo anterior. Inclui o mapa de endereços: a janela do EL0 não pode ocupar a faixa onde o firmware aloca framebuffers grandes | Testes cobrem sucesso, rejeição de modo inválido, falha de alocação e conflito com a janela EL0; todos os modos da tabela aplicam | Muda entre pelo menos dois modos aceitos pelo conjunto monitor/firmware, inclusive HD e Full HD, e recupera para o modo anterior se necessário |
| **25. Reconstrução do framebuffer após mudança** | Atualização coordenada de buffers, escala/área de trabalho, cursor e superfícies quando a resolução muda | Testes redimensionam repetidamente e detectam acesso fora dos limites, vazamentos e artefatos | Trocas sucessivas de resolução mantêm a imagem íntegra e não deixam o sistema sem saída |
| **26. Eventos de entrada unificados** | Eventos de teclado e mouse: teclas, modificadores, movimento, botões e roda; fila e entrega a uma aplicação focada | Injeção de eventos testa ordenação, perda, foco e limites das filas | Teclado e mouse USB funcionam na GUI; eventos aparecem no shell gráfico e nos controles |
| **27. Protocolo de janelas e superfícies** | API documentada para criar/destruir janela, definir título, solicitar tamanho, desenhar e receber eventos | Programa de teste cria uma janela, desenha e fecha sem acessar diretamente o framebuffer global | O mesmo cliente de teste abre e fecha janelas no Pi; falhas do cliente não derrubam o compositor |
| **28. Compositor e desenho de janelas** | Composição de superfícies, fundo, bordas, título, recorte, z-order, foco e atualização parcial da tela | Testes de pixels e captura de framebuffer verificam sobreposição, recorte, foco e redraw | Verificação visual em monitor confirma a composição; janelas sobrepostas são redesenhadas corretamente |
| **29. Movimentação, redimensionamento e foco** | Arrastar janelas, mudar tamanho quando permitido, ativar foco e trazer janela à frente; atalhos básicos | Testes de eventos reproduzidos verificam coordenadas, foco e limites do desktop | Usuário move, redimensiona e alterna foco com mouse/teclado em aplicações reais |
| **30. API e ciclo de vida de aplicações gráficas** | Bibliotecas/serviços comuns para iniciar, suspender, fechar e reportar erro de aplicações; relação entre processo e janela | Aplicações de teste abrem simultaneamente, uma termina e as demais continuam | Shell, editor e outras aplicações executam em conjunto; fechar uma aplicação não encerra as outras |
| **31. Console gráfico e shell em janela** | Shell funcional dentro de janela, com entrada, histórico, edição de linha, saída rolável, redimensionamento e execução de programas | Roteiro automatizado executa comandos, redireciona/mostra saída conforme suportado e valida histórico | Shell aceita comandos por teclado USB, executa programas instalados e continua responsivo com outras janelas abertas |
| **32. Gerenciador de aplicações e multitarefa interativa** | Inicialização de diversos programas de usuário, alternância por foco/atalho e tratamento de aplicação bloqueada ou encerrada | Teste abre vários clientes e verifica que cada um recebe apenas seus eventos e conserva estado | Shell, editor e utilitários permanecem ativos ao alternar entre janelas sem congelar o desktop |
| **33. Barra de tarefas** | Barra persistente com botão/menu de aplicações, indicadores de janelas abertas, janela ativa e relógio/estado básico | Testes verificam lista, ativação, minimização/restauração e atualização de horário | Barra permanece visível e seus botões alternam/restauram janelas no monitor do Pi |
| **34. Minimizar, restaurar e fechar janelas** | Estados de janela, botões/atalhos e comportamento definido ao encerrar a aplicação; atualização da barra de tarefas | Transições repetidas não perdem referências nem deixam superfícies obsoletas | Minimizar/restaurar/fechar shell e editor funciona sem falhas e sem bloquear o desktop |
| **35. Serviço de arquivos para aplicações** | API de arquivos com caminhos, diretórios, erros, permissões simples e operações necessárias aos programas | Testes criam, leem, alteram e removem arquivos em disco virtual; erros são exibidos corretamente | Operações equivalentes persistem no SD real; desligamento/reinício não perde gravações confirmadas |
| **36. Explorador de arquivos — navegação** | Aplicação para listar pastas/arquivos, navegar por diretórios, atualizar lista e exibir caminho/local atual | Testes com árvore de arquivos simulada cobrem diretório vazio, nomes, erros e navegação | Explora o cartão SD e mostra arquivos reais, inclusive com nomes e tamanhos variados |
| **37. Explorador de arquivos — operações básicas** | Abrir arquivo em aplicação associada, criar pasta, renomear, copiar, mover e excluir com confirmação | Testes cobrem sucesso, destino existente, falta de espaço e cancelamento; exclusão exige confirmação | Operações verificadas no SD real; não apaga silenciosamente e mantém o sistema inicializável |
| **38. Editor de texto — edição básica** | Aplicação com área de edição, cursor, seleção simples, inserir/apagar texto, quebra de linha e rolagem | Testes editam texto longo, linhas vazias e limites de buffer; layout permanece correto | Edição por teclado USB funciona na janela e permanece responsiva com outras janelas abertas |
| **39. Editor — abrir, salvar e estado alterado** | Abrir/salvar arquivos de texto, novo documento, salvar como, indicação de alterações não salvas e confirmação ao fechar | Testes de ida-e-volta preservam conteúdo UTF-8 definido no escopo; falhas de escrita não descartam o buffer | Arquivos salvos podem ser reabertos após reinício; fechamento de documento alterado pede confirmação |
| **40. Painel de controle — tela** | Tela de configurações para resolução e modo de vídeo, aplicar/cancelar e voltar ao modo seguro com confirmação | Testes simulam modos suportados, rejeição e timeout; opção inválida não fica persistida | Usuário muda resolução via painel, confirma a imagem ou deixa expirar a confirmação para voltar ao modo anterior |
| **41. Painel de controle — preferências básicas** | Configurar data/hora quando suportado, tema/cores, escala ou tamanho de fonte e comportamento simples do desktop; salvar preferências | Testes validam persistência, valores inválidos e configuração padrão restaurável | Preferências continuam após reinício e podem ser restauradas sem impedir o boot gráfico |
| **42. Menu de aplicações e fluxo de trabalho** | Menu na barra para iniciar shell, explorador, editor e painel; atalhos e associação de texto ao editor | Cada item inicia o programa correto e reporta indisponibilidade de forma compreensível | Os quatro programas iniciam pelo menu e continuam independentes em janelas distintas |
| **43. Mensagens, diálogos e notificações** | Diálogos comuns para erro, confirmação, salvar alterações e falta de espaço; foco e captura de interação previsíveis | Testes verificam botões, tecla Escape/Enter e que o diálogo não bloqueia processos alheios | Mensagens são legíveis no monitor e teclado/mouse respondem corretamente; erros não encerram o desktop |
| **44. Inicialização do ambiente gráfico** | Gerenciador de sessão simples que inicia compositor, barra e serviços; modo de recuperação para iniciar apenas shell serial | Boot automatizado chega ao desktop; falha de um componente retorna a um ambiente de diagnóstico | Cartão inicializa diretamente no desktop, e uma falha gráfica ainda deixa acesso ao shell serial/recuperação |
| **45. Estabilidade integrada e experiência final** | Teste conjunto prolongado, consumo de memória, carga de redraw, troca de resolução, operações de arquivos e encerramentos anormais; correções de usabilidade | Testes de regressão passam após reinicializações e sob carga; artefatos visuais e falhas ficam registrados | Sessão prolongada no Pi com shell, explorador, editor e painel abertos; sem corrupção, congelamento ou perda de recuperação |
| **46. Lançamento do ambiente gráfico** | Versão identificada, imagem de SD reproduzível, instruções, lista de limitações conhecidas e procedimento de recuperação/atualização | Imagem limpa inicia o ambiente e passa os testes de aceitação documentados | Cartão preparado do zero inicia no Pi 3B+, permite mudar resolução, abrir várias janelas e usar os quatro programas principais |

## Definição de concluído para as funcionalidades solicitadas

- **Controle de resolução:** exibir os modos realmente suportados pelo monitor/firmware, aplicar o modo escolhido com segurança e recuperar automaticamente se a imagem não for confirmada.
- **Várias janelas e programas:** manter processos independentes, janelas sobrepostas, foco, movimentação, minimização/restauração e fechamento sem que uma aplicação trave as demais.
- **Shell em janela:** aceitar comandos e executar aplicações do próprio sistema, sem remover o shell serial de diagnóstico.
- **Programas iniciais:** painel de controle, explorador de arquivos, barra de tarefas e editor de texto disponíveis pelo menu e utilizáveis simultaneamente.
- **Sistema novo:** o kernel, os serviços, o compositor e os aplicativos são do projeto; o firmware de boot da placa pode permanecer como carregador inicial, sem transformar o projeto em uma distribuição Linux.

## Critérios para declarar o complemento finalizado

O complemento estará concluído somente quando o marco 46 passar no QEMU e no Raspberry Pi real, e os seguintes cenários forem demonstrados no próprio Pi:

1. Iniciar no desktop e abrir shell, explorador, editor e painel de controle sem encerrar os demais.
2. Mover, redimensionar, focar, minimizar, restaurar e fechar janelas; alternar programas pela barra de tarefas.
3. Criar, abrir, editar, salvar, renomear e excluir um arquivo de texto, com confirmação para operações destrutivas ou alterações não salvas.
4. Alterar a resolução para um modo suportado, confirmar que a tela está visível e verificar retorno seguro se a alteração não for confirmada.
5. Reiniciar o sistema e confirmar persistência dos arquivos e das preferências básicas, mantendo um caminho de recuperação pela serial.

A entrega é considerada um **ambiente gráfico viável para o escopo do projeto**, não um substituto geral de desktops comerciais. A compatibilidade é inicialmente limitada ao hardware e aos modos efetivamente testados no Raspberry Pi 3B+; novos dispositivos e recursos podem ser acrescentados em marcos posteriores.

## Notas de projeto para manter o escopo realizável

- Implementar primeiro um desktop de resolução fixa e uma janela de teste; acrescentar mudança de modo somente depois que composição, redimensionamento de buffers e tratamento de falhas estiverem sólidos.
- Preferir um único compositor/servidor gráfico com API pequena e explícita. Clientes não devem escrever diretamente no framebuffer compartilhado nem acessar a memória de outras aplicações.
- Executar aplicativos em espaço de usuário sempre que o modelo do kernel permitir. Uma falha de cliente deve fechar ou reiniciar aquele cliente, não reinicializar o sistema inteiro.
- Manter o console serial como canal de diagnóstico e recuperação, mesmo depois que o desktop iniciar automaticamente.
- Usar fontes bitmap e controles simples no primeiro lançamento; suporte amplo a fontes, aceleração gráfica, rede, áudio, USB avançado e compatibilidade com protocolos gráficos externos ficam fora do escopo inicial.
- Não prometer uma lista universal de resoluções: a escolha disponível depende do monitor, do firmware/configuração de boot e do modo de vídeo implementado. Mostrar apenas modos que foram consultados e validados.

## Estado atual

- **Marcos 1–21 (plano-base): concluídos.** Marco 21 aceito no Pi em
  2026-11-07: a tela mostrou `rasppios marco 21` com os painéis completos.
  Aceite físico pendente apenas para o teclado USB (ver pendência abaixo).
  O kernel sobe em AArch64 `no_std`, passa por EL2/EL3 para EL1, usa miniUART,
  exceções, timer de 100 Hz, MMU com identidade, alocador físico, trava entre
  quatro núcleos, escalonador cooperativo e preemptivo, EL0 com chamadas de
  sistema, teclado USB HID, SDHOST, RPFS com v1/v2, journal e `fsck`,
  carregador `RPEX`, shell, framebuffer 640x480 RGB565, GUI com três painéis
  e a imagem de distribuição. `make check` valida boot duplo, console,
  persistência, `fsck: ok`, `stress: ok 100`, falhas controladas, `versao`,
  `gfx`, `modos` e a imagem de distribuição. Detalhes por marco estão no
  `README.md`.

**Pendência aberta — teclado USB no Pi físico (2026-11-07).**
**Resolvido em 2026-11-07:** a UART do Pi passou a mostrar `usb-host-ready`,
`usb-hub-found`, `teclado: hid` e `usb-keyboard-hid`, com o teclado
funcionando na GUI. O histórico abaixo fica como registro do diagnóstico.

Histórico do diagnóstico (anterior à correção):

| Linha | Valor | Leitura |
|---|---|---|
| `usb` | `21405 sp 1` | conectado, `PRTENA`, `PRTPWR`, full-speed |
| `hc` | `0 c c0000008` | `HCINT` zero; canal com `CHENA` e `CHDIS` presos |
| `cfg` | `20002400 h 200000` | `FORCEHOSTMODE`, `TRDTIM=9`, PHY UTMI+ |
| `fr` | `4 to 40` | ~1 kHz de SOF; clock do PHY ativo; 40 timeouts |

Hipóteses já refutadas no hardware: PHY ULPI, PHY UTMI, espera extra após o
reset, `halt_channel` com `CHDIS`, bit `MC` em controle, alias de barramento
`0xC0000000` no `HCDMA` e inversão da ordem USB antes do vídeo. Nenhuma mudou
`HCINT = 0`. O mesmo driver funcionou no Pi no marco 13, antes da GUI existir.

**Marco 22 (contrato gráfico): QEMU feito; aceite físico pendente.** O
contrato vive em `src/gfx.rs` como código verificável: `PixelFormat::Rgb565`
com 2 bytes por pixel, limites (`1920x1080`, 8 superfícies, 8 janelas, título
de 32 bytes), validação de `Mode` (largura, altura, stride mínimo e alinhado,
`buffer_len`), `Rect` com interseção e teste de ponto, `Surface::validate`
contra o tamanho da arena e as mensagens da API (`create`, `destroy`, `move`,
`resize`, `raise`, `damage`, `title`, `close`) mais os eventos (`key`,
`pointer`, `focus`, `close`). Um cliente nunca recebe o endereço do
framebuffer: recebe `Surface` com deslocamento na arena do serviço.
O comando `gfx` roda o autoteste sem tocar no framebuffer e imprime
`gfx: contrato ok`; `make check` exige essa linha. O próprio autoteste
encontrou dois defeitos reais: `create` valida largura e altura em `c,d` e
`Message` ocupa 20 bytes, não 24.

**Marco 24 (alteração de resolução segura): QEMU feito; aceite físico
pendente.** `video::init_mode(mode)` valida o modo pelo contrato de `gfx`,
negocia com o firmware, remapeia o buffer e só então publica o estado. Se o
pedido falha, ele tenta renegociar o modo anterior; se nem isso funciona,
marca o vídeo como indisponível em vez de deixar a tela apontando para um
buffer inválido. `modes::apply` guarda o modo anterior e entrega um `Pending`
com prazo de `CONFIRM_TICKS` (150 ticks, 1,5 s a 100 Hz): se o usuário não
confirmar, `expired()` permite que o modo anterior volte sozinho — cobre o
caso em que o monitor não sincroniza e a pessoa não enxerga nada para
digitar.

O comando `resolucao` sem argumento mostra o modo ativo; `resolucao N` aplica
o modo de índice `N` da tabela do Marco 23. A linha seguinte digitada
confirma (ou `cancela` restaura), independentemente do comando. A GUI passou
a calcular o layout a partir das dimensões reais (`Layout::for_screen`), com
painéis proporcionais e limites de linha por painel; em 640x480 o resultado é
idêntico ao anterior.

Ciclo validado no QEMU: `640x480` → `resolucao 1` → confirma → `800x600` →
`resolucao 0` → `cancela` → volta a `800x600`. Dois defeitos corrigidos no
caminho: `resolve_pending` recebia só o argumento do comando e não a linha
inteira, então `cancela` nunca era reconhecido; e o `reply` precisava
consumir a linha de confirmação antes de interpretá-la como comando.

**Bug encontrado no Pi.** `resolucao 1` respondia `modo nao suportado`
logo após o boot. A causa: `is_supported()` consultava uma máscara em cache
que só era preenchida por `discover()`, chamado por `modos`. Sem rodar
`modos` antes, a máscara estava zerada e todo modo era recusado. A correção
calcula `VALID_MASK` em tempo de compilação a partir da própria tabela, sem
depender do firmware nem de ordem de comandos. O teste automatizado passou a
executar a troca real **antes** de `modos`, e o ciclo red-green foi
comprovado: com o bug reintroduzido, `make check` falha em
`timeout esperando b'resolucao: 800x600 aplicada'`; com a correção, passa.

**Teclado USB no Pi — resolvido.** A saída da UART do Pi mostra
`usb-host-ready`, `usb-hub-found`, `teclado: hid` e `usb-keyboard-hid`.
A pendência de enumeração descrita abaixo deixou de ocorrer.

**Marco 25 (mapa de endereços do framebuffer e modos altos): QEMU feito;
aceite físico pendente.** A janela do programa de usuário ficava em
`0x3C200000`, exatamente na faixa onde o firmware aloca framebuffers maiores
que 800x600 (`0x3C000000..0x3C280000`). Por isso `resolucao 2` em diante
respondia `modo nao suportado` ou `falhou`: o buffer novo caía sobre a
própria página de código do EL0 e `mmu::map_framebuffer` recusava com razão.

A janela do EL0 foi movida para `0x3D000000`, no vão livre entre a RAM e o
MMIO. A asserção em tempo de compilação `USER_L2 == 488` e a nova
`USER_CODE_VA >= 0x3D00_0000` impedem que a janela volte para a faixa do
framebuffer. `init_mode` também passou a liberar o buffer anterior com
`RELEASE_FRAMEBUFFER` antes de pedir outro, e a diagnosticar o conflito com a
janela EL0 em vez de falhar em silêncio.

Resultado no QEMU: **todos os seis modos aplicam** — `640x480`, `800x600`,
`1024x768`, `1280x720` (HD), `1280x1024` e `1920x1080` (Full HD) — e as
regressões do EL0 continuam passando (`usuario: el0`, `proprio: ok`,
`usuario: kernel negado`, `roda`/`app: ok`). O `make check` exercita a troca
para todos os modos e volta ao modo seguro.

**Boot em Full HD com fallback.** `video::BOOT_MODES` tenta `1920x1080`,
depois `1280x720` e por fim `640x480`. Se o firmware recusar um modo, o
kernel registra `video: WxH recusado; tentando o proximo` e segue para o
seguinte, sem intervenção. No QEMU o boot entrega `1920x1080`; o fallback foi
comprovado forçando dois modos inválidos, que caíram para `640x480`.

A janela de confirmação de `resolucao N` passou de 1,5 s para **10 s**
(`CONFIRM_TICKS = 1000`) e a expiração agora é verificada **durante** a
espera, em `read_line`. Antes ela só era detectada quando o usuário digitava
algo, então o comando seguinte expirava a troca em vez de confirmá-la — o
sintoma observado no Pi. O comando `tela` e a cena de `video::exercise`
passaram a acompanhar a resolução real em vez de assumir 640x480.

**Marco 25 (reconstrução do framebuffer após mudança): QEMU feito; aceite
físico pendente.** `mmu::map_framebuffer` passou a **restaurar a tradução
anterior** antes de mapear o buffer novo. Antes, cada troca de resolução
deixava as páginas do framebuffer antigo mapeadas como memória normal para
sempre: a função só reescrevia os blocos que tocava, então faixas que
deixavam de ser framebuffer ficavam privilegiadas e acessíveis sem motivo.
Agora `FB_START`/`FB_END` guardam o intervalo em vigor, `restore_previous()`
devolve cada bloco ao descritor original e `unmap_framebuffer()` é usado
quando o vídeo termina indisponível, para não sobrar mapeamento órfão.

O `make check` alterna `1920x1080` e `640x480` três vezes, aplica
`1280x720` e exige `tela: ok 1280x720 rgb565` no fim — sete trocas
consecutivas sem artefato, sem perder a GUI e sem acesso fora dos limites.
No Pi, o boot já foi confirmado em Full HD em 2026-11-07.

Mídia de teste: `/dev/sda` de 29,7 GB, cartão SD em adaptador USB, partição
`/dev/sda1` em FAT32 montada em `/media/vladimir/PENDRIVE`. `make write`
copia os cinco arquivos sem formatar; depois de gravar, `sync` e
`udisksctl unmount -b /dev/sda1` antes de retirar o cartão, e então
`make preview` para ver no QEMU o que o Pi deve mostrar.

**Marco 23 (descoberta de modos): QEMU feito; aceite físico pendente.** A
consulta ao firmware vive em `src/modes.rs`. Ela pergunta as dimensões e o
stride correntes ao mailbox de propriedades e mantém uma tabela
conservadora de seis modos RGB565 (`640x480`, `800x600`, `1024x768`,
`1280x720`, `1280x1024`, `1920x1080`), ordenada por área, com o menor como
modo seguro. O comando `modos` imprime o modo atual, quantos modos da tabela
são aceitos e, para cada um, `stride` e tamanho do buffer, marcando com `*`
o que está ativo. Executa também o autoteste, que valida a tabela pelo
contrato de `gfx`, a ordem crescente, o modo seguro, a rejeição de índice
inválido e o limite de `MAX_MODES`. `make check` exige
`modos: descoberta ok`.

Dois defeitos reais encontrados e corrigidos na implementação: a tag de
leitura é `0x0004_0003` (`GET_PHYSICAL_WIDTH_HEIGHT`), não `0x0004_8003`,
que é a de escrita; e o código de resposta do mailbox fica na palavra 4 do
buffer, não na 3, que é o tamanho da tag. No QEMU o resultado é
`modos: atual 640x480 stride 1280` com 6 de 6 modos aceitos.

## Fontes de referência já usadas no plano-base

- [Raspberry Pi — especificações do 3B+](https://www.raspberrypi.com/products/raspberry-pi-3-model-b-plus)
- [Arm — níveis de exceção](https://learn.arm.com/learning-paths/embedded-and-microcontrollers/bare-metal/exception-levels)
- [QEMU — modelos Raspberry Pi](https://www.qemu.org/docs/master/system/arm/raspi.html)
- [OpenStax — What Is an Operating System?](https://openstax.org/books/introduction-computer-science/pages/6-1-what-is-an-operating-system)
- [Pearson — Modern Operating Systems, 5th edition](https://www.pearson.com/en-us/subject-catalog/p/modern-operating-systems/P200000003295/9780137618880)