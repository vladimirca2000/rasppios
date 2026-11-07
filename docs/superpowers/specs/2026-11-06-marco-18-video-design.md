# Marco 18 — saída gráfica

## Objetivo

Adicionar uma saída gráfica mínima ao rasppios sem remover o console UART:

- solicitar um framebuffer ao firmware pelo mailbox de propriedades;
- usar 640×480 pixels em RGB565;
- preencher a tela e desenhar retângulos;
- desenhar texto com uma fonte bitmap pequena;
- expor o teste por um comando `tela`.

O aceite visual será a imagem no monitor do QEMU e no Raspberry Pi 3B+.

## Arquitetura

Será criado um módulo `src/video.rs` com uma interface pequena:

- `init()` solicita o framebuffer e valida largura, altura, profundidade,
  endereço e stride retornados;
- `clear(color)` preenche a área visível;
- `rect(x, y, width, height, color)` desenha um retângulo limitado à tela;
- `text(x, y, text, color, background)` desenha caracteres da fonte bitmap;
- `exercise()` executa a cena estável do comando `tela`.

O mailbox continuará usando os offsets já documentados no `physmem.rs` e em
`registradores.md`. A mensagem será alinhada em 16 bytes, permanecerá em uma
área estática e converterá o endereço ARM para o alias de barramento exigido
pelo firmware. O framebuffer retornado será tratado como memória normal
cacheável pelo código, com limpeza de cache antes de devolver o controle ao
firmware/monitor quando necessário.

O módulo não assumirá que o stride é igual à largura. Cada linha usará o
stride retornado. O pixel RGB565 será escrito em little-endian e os limites
serão validados antes de qualquer acesso ao buffer.

## Cena de teste

O comando `tela`:

1. inicializa o framebuffer uma vez;
2. limpa a tela com fundo azul-escuro;
3. desenha uma faixa e três retângulos em cores contrastantes;
4. desenha texto bitmap com o título `rasppios marco 18` e linhas de estado;
5. imprime na UART `tela: ok <larguraxaltura> rgb565`.

Se o firmware não entregar um framebuffer compatível, o comando imprime
`tela: indisponivel` e o kernel continua no prompt sem acessar memória inválida.

## Integração

`main.rs` inicializará o vídeo depois da UART e antes do console, mas uma falha
de vídeo não impedirá o boot. `console.rs` adicionará `tela` ao help e ao
despacho. O caminho de entrada USB/UART permanece inalterado.

O mapeamento atual da MMU será estendido somente para a região devolvida pelo
mailbox, se ela estiver fora do mapa de RAM já existente. A região será
gravável, não executável e não será exposta ao EL0. Nenhuma aplicação ou
syscall será alterada neste marco.

## Erros e invariantes

- largura e altura devem ser positivos e caber nos limites suportados;
- profundidade deve ser 16 bits;
- stride deve ser suficiente para uma linha de `width` pixels;
- endereço e tamanho devem ser alinhados e caber na memória ARM disponível;
- coordenadas e dimensões serão recortadas ou rejeitadas antes dos loops;
- ausência de framebuffer não pode gerar panic nem abortar o console;
- a UART continuará sendo a fonte de diagnóstico mesmo com vídeo ativo.

## Verificação

- `make check` continuará validando o boot e a sequência do shell;
- QEMU será iniciado com janela gráfica e `-serial vc`, e a aba de framebuffer
  deverá mostrar a cena enquanto `serial1` deverá mostrar `tela: ok`;
- o Pi 3B+ será testado no monitor físico, registrando resolução, formato e
  mensagem UART;
- `agent.md` e `README.md` serão atualizados com o resultado do marco.
