# Relatório da Task 4 — Marco 18

## Correções da revisão

- O framebuffer é memória normal cacheável: `video::exercise()` limpa as
  linhas de cache do intervalo retornado com `dc cvac`, alinhamento de 64
  bytes e `dsb sy` antes de devolver o console. A negociação rejeita
  explicitamente tamanhos de framebuffer que não sejam múltiplos da linha de
  cache; o tamanho QEMU `640x480x2` permanece aceito.
- A mídia atual é `/dev/sda1` em FAT32 (`vfat`), montada em
  `/media/vladimir/PENDRIVE`. `make write` apenas copia os arquivos e não
  formata. A conversão exFAT→FAT32 ocorreu uma única vez antes dos testes e
  foi registrada separadamente.
- `make image` foi documentado como imagem de teste exFAT, conforme
  `scripts/image.sh`; nenhum script foi alterado.
- A saída esperada do kernel atual foi corrigida de `rasppios: marco 15` para
  `rasppios: marco 17`.
- O aceite permanece: QEMU feito, Pi aberto. O vídeo físico ainda aguarda
  teste no monitor do Pi 3B+.

## Status

DONE — documentação e registro de aceite atualizados.

## Commit

`2b0d83d docs: corrige achados da revisão da task 4`.

## Arquivos

- `agent.md`: Marco 18 registrado como QEMU feito e Pi aberto, com cena,
  resolução, formato, stride e comando de reprodução.
- `README.md`: seção do Marco 18 e reprodução visual do QEMU adicionadas.
- `.superpowers/sdd/task-4-report.md`: este relatório.
- `.cursor/skills/rasppios-bcm2837/registradores.md`: não alterado; não houve
  fato novo de registrador efetivamente usado nesta tarefa.

## Testes e validações

- `make check`: iniciado uma vez, mas ficou preso em
  `bash scripts/check-qemu.sh` nesta execução do ambiente; foi encerrado sem
  resultado final. Não foi executado novamente.
- `git diff --check`: passou.
- Lint do editor para os arquivos documentais: nenhum erro.
- `findmnt -n -o SOURCE,FSTYPE,TARGET /media/vladimir/PENDRIVE`: confirmou
  `/dev/sda1 vfat /media/vladimir/PENDRIVE`.
- `scripts/image.sh`: conferido; continua usando `mkfs.exfat`.
- Reprodução visual QEMU: confirmada anteriormente nas Tasks 1–3, com
  `video: 640x480 rgb565 stride 1280` e `tela: ok 640x480 rgb565`.

## Cópia ao cartão

Sim. `make write DEST=/media/vladimir/PENDRIVE` concluiu com sucesso, sem
formatar, copiando os cinco arquivos de boot para `/dev/sda1` em FAT32.

## Concerns

- O framebuffer ainda não foi confirmado no monitor de um Raspberry Pi 3B+
  físico; o aceite do Pi permanece aberto.
- A execução atual de `make check` não produziu resultado final porque o
  processo QEMU/harness ficou preso no ambiente; isso deve ser revalidado em
  um ambiente em que o encerramento do QEMU funcione.

## Correção final da Task 4

- `agent.md` agora descreve fielmente `scripts/install-boot.sh`: `make write`
  aceita `vfat` e `exfat`, não formata e recusa dispositivos `nvme` e
  `mmcblk0`.
- A documentação FAT32 atual do cartão foi preservada.
- `git diff --check`: passou.
- Lint de `agent.md`: nenhum erro.
- Commit: correção documental registrada no histórico.

## Correção dos achados Important

- `cargo build --release`: passou; permanece o warning preexistente de
  `BLACK` não usado.
- `git diff --check`: passou.
- Lint de `src/video.rs` e deste relatório: nenhum erro.
- `make check`: executado, mas o primeiro QEMU de `scripts/check-qemu.sh`
  ficou preso consumindo CPU e não produziu saída final; foi encerrado sem
  resultado. O bloqueio é o mesmo comportamento de QEMU já registrado acima,
  não um aceite inventado.

## Correção do achado restante da revisão

- `src/video.rs` agora rejeita o endereço ARM final do framebuffer quando ele
  não está alinhado a 64 bytes, depois da remoção do alias BUS. A validação
  existente de endereço mailbox alinhado a 16 bytes e de tamanho múltiplo de
  64 foi preservada; o endereço retornado pelo QEMU continua dentro do
  contrato aceito.
- `cargo build --release`: passou; permanece o warning preexistente de
  `BLACK` não usado.
- `git diff --check`: passou.
- Lint de `src/video.rs` e deste relatório: nenhum erro.
- `make check`: reproduziu o bloqueio ambiental do harness/QEMU, sem produzir
  resultado final; o processo foi encerrado e isso não foi contado como
  aceite.
